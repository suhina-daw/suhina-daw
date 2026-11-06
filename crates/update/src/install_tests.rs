// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::Write;

use super::*;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-update-{name}-{}", std::process::id()));
    let _removed = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    dir
}

/// A package as the release workflow makes it: one top folder with the program in it.
fn package_zip(path: &Path, top: &str, program: &[u8]) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file(format!("{top}/{}", program_name()), options).unwrap();
    zip.write_all(program).unwrap();
    zip.start_file(format!("{top}/LICENSE"), options).unwrap();
    zip.write_all(b"GPL").unwrap();
    zip.finish().unwrap();
}

fn package_tar(path: &Path, top: &str, program: &[u8]) {
    let gz = flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::fast());
    let mut tar = tar::Builder::new(gz);
    let mut header = tar::Header::new_gnu();
    header.set_size(program.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(&mut header, format!("{top}/{}", program_name()), program).unwrap();
    tar.into_inner().unwrap().finish().unwrap();
}

#[test]
fn the_checksum_is_the_files_sha256() {
    let dir = folder("sha");
    let file = dir.join("abc");
    std::fs::write(&file, b"abc").unwrap();
    assert_eq!(sha256(&file).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn a_package_unpacks_to_the_folder_with_the_program() {
    let dir = folder("unpack");
    let archive = dir.join("package.zip");
    package_zip(&archive, "suhina-nightly-windows-x86_64", b"new");
    let into = dir.join("zip");
    std::fs::create_dir(&into).unwrap();
    let found = unpack(&archive, &into).unwrap();
    assert_eq!(std::fs::read(found.join(program_name())).unwrap(), b"new");

    let archive = dir.join("package.tar.gz");
    package_tar(&archive, "suhina-nightly-linux-x86_64", b"newer");
    let into = dir.join("tar");
    std::fs::create_dir(&into).unwrap();
    let found = unpack(&archive, &into).unwrap();
    assert_eq!(std::fs::read(found.join(program_name())).unwrap(), b"newer");

    // A test build's artifact: a zip around the .tar.gz.
    let artifact = dir.join("artifact.zip");
    let mut zip = zip::ZipWriter::new(File::create(&artifact).unwrap());
    zip.start_file("suhina-pr663-linux-x86_64.tar.gz", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(&std::fs::read(&archive).unwrap()).unwrap();
    zip.finish().unwrap();
    let into = dir.join("artifact");
    std::fs::create_dir(&into).unwrap();
    let found = unpack(&artifact, &into).unwrap();
    assert_eq!(std::fs::read(found.join(program_name())).unwrap(), b"newer");

    std::fs::write(dir.join("page.html"), b"<html>").unwrap();
    assert!(unpack(&dir.join("page.html"), &dir).is_err());
}

#[test]
fn a_macos_package_unpacks_to_the_program_in_its_app_bundle() {
    let dir = folder("unpack-bundle");
    let archive = dir.join("package.tar.gz");
    let top = "suhina-nightly-macos-universal";
    package_tar(&archive, &format!("{top}/Suhina.app/Contents/MacOS"), b"bundled");
    let into = dir.join("tar");
    std::fs::create_dir(&into).unwrap();
    let found = unpack(&archive, &into).unwrap();
    assert_eq!(found, into.join(top).join("Suhina.app/Contents/MacOS"));
    assert_eq!(std::fs::read(found.join(program_name())).unwrap(), b"bundled");
    assert_eq!(bundle_of(&found), Some(into.join(top).join("Suhina.app")));
    assert_eq!(bundle_of(&into.join(top)), None);
    assert_eq!(bundle_of(&into.join("Other/Contents/MacOS")), None);

    // The program under another Contents folder is no app bundle Suhina can run: no update.
    let archive = dir.join("package-misplaced.tar.gz");
    package_tar(&archive, &format!("{top}/Suhina.app/Contents/Resources"), b"misplaced");
    let into = dir.join("misplaced");
    std::fs::create_dir(&into).unwrap();
    assert!(
        unpack(&archive, &into).is_err_and(|e| e.contains(&format!("no {}", program_name()))),
        "a program outside Contents/MacOS is not found"
    );
}

#[test]
fn a_bundle_is_replaced_whole_and_the_old_one_deleted_on_the_next_start() {
    let dir = folder("replace-bundle");
    let make = |at: &Path, program: &[u8]| {
        let macos = at.join("Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::write(macos.join(program_name()), program).unwrap();
        std::fs::write(at.join("Contents/Info.plist"), program).unwrap();
        macos
    };
    let running = make(&dir.join("Suhina.app"), b"old");
    std::fs::write(running.join("left-from-the-old-one"), b"").unwrap();
    // Another app beside it, and its own `.old`, which the clean-up leaves alone.
    std::fs::create_dir_all(dir.join("Other.app.old")).unwrap();
    let staging = staging_folder(&running);
    assert_eq!(staging, dir.join(STAGING));
    let new = make(&staging.join("unpacked/Suhina.app"), b"new");

    replace_bundle(&bundle_of(&new).unwrap(), &bundle_of(&running).unwrap()).unwrap();
    assert_eq!(std::fs::read(running.join(program_name())).unwrap(), b"new");
    assert_eq!(std::fs::read(dir.join("Suhina.app/Contents/Info.plist")).unwrap(), b"new");
    assert!(!running.join("left-from-the-old-one").exists());
    assert!(dir.join("Suhina.app.old").is_dir());

    clean_up(&running);
    assert!(!dir.join("Suhina.app.old").exists());
    assert!(!staging.exists());
    assert!(dir.join("Other.app.old").is_dir());
    assert_eq!(std::fs::read(running.join(program_name())).unwrap(), b"new");
}

#[test]
fn a_bundle_that_cannot_be_moved_in_leaves_the_running_one_in_place() {
    let dir = folder("replace-bundle-fails");
    let running = dir.join("Suhina.app/Contents/MacOS");
    std::fs::create_dir_all(&running).unwrap();
    std::fs::write(running.join(program_name()), b"old").unwrap();
    assert!(replace_bundle(&dir.join("missing/Suhina.app"), &dir.join("Suhina.app")).is_err());
    assert_eq!(std::fs::read(running.join(program_name())).unwrap(), b"old");
    assert!(!dir.join("Suhina.app.old").exists());
}

#[test]
fn a_package_without_an_app_bundle_is_refused_while_the_running_one_is_a_bundle() {
    let dir = folder("put-in-place");
    let running = dir.join("Suhina.app/Contents/MacOS");
    std::fs::create_dir_all(&running).unwrap();
    std::fs::write(running.join(program_name()), b"old").unwrap();

    // No bundle in the download: refused, and the running bundle is untouched — its files cannot
    // go inside the bundle's signature.
    let plain = dir.join("plain");
    std::fs::create_dir(&plain).unwrap();
    std::fs::write(plain.join(program_name()), b"new").unwrap();
    let refused = put_in_place(&plain, &running).unwrap_err();
    assert!(refused.contains("no Suhina.app"), "{refused}");
    assert_eq!(std::fs::read(running.join(program_name())).unwrap(), b"old");
    assert!(!dir.join("Suhina.app.old").exists());

    // A bundle takes a bundle's place whole.
    let new = dir.join("New.app/Contents/MacOS");
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(new.join(program_name()), b"new").unwrap();
    put_in_place(&new, &running).unwrap();
    assert_eq!(std::fs::read(running.join(program_name())).unwrap(), b"new");
    assert!(dir.join("Suhina.app.old").is_dir(), "the old bundle is aside");

    // Without a bundle running, the files go in place one by one.
    let bare = dir.join("bare");
    std::fs::create_dir(&bare).unwrap();
    std::fs::write(bare.join("kept.txt"), b"kept").unwrap();
    put_in_place(&running, &bare).unwrap();
    assert_eq!(std::fs::read(bare.join(program_name())).unwrap(), b"new");
    assert_eq!(std::fs::read(bare.join("kept.txt")).unwrap(), b"kept");
}

#[test]
fn replacing_puts_each_file_in_place_and_cleaning_up_removes_what_was_moved_aside() {
    let dir = folder("replace");
    let program = dir.join("program");
    std::fs::create_dir(&program).unwrap();
    std::fs::write(program.join(program_name()), b"old").unwrap();
    std::fs::write(program.join("settings-of-its-own.txt"), b"kept").unwrap();
    let new = dir.join("new");
    std::fs::create_dir(&new).unwrap();
    std::fs::write(new.join(program_name()), b"new").unwrap();
    std::fs::write(new.join("LICENSE"), b"GPL").unwrap();

    assert_eq!(replace(&new, &program).unwrap(), 2);
    assert_eq!(std::fs::read(program.join(program_name())).unwrap(), b"new");
    assert_eq!(std::fs::read(program.join("LICENSE")).unwrap(), b"GPL");
    assert_eq!(std::fs::read(program.join("settings-of-its-own.txt")).unwrap(), b"kept");
    std::fs::create_dir(program.join(STAGING)).unwrap();
    clean_up(&program);
    let mut left: Vec<String> =
        list(&program).unwrap().iter().filter_map(|p| p.file_name()?.to_str().map(String::from)).collect();
    left.sort();
    let mut expected = vec!["LICENSE".to_string(), program_name().to_string(), "settings-of-its-own.txt".into()];
    expected.sort();
    assert_eq!(left, expected);
}
