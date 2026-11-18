// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn a_thread_starts_as_the_host() {
    let started = std::thread::spawn(|| {
        set(Platform::Windows);
        current()
    })
    .join()
    .unwrap();
    assert_eq!(started, Platform::Windows);
    // Another thread's setting leaves this one's alone.
    assert_eq!(current(), Platform::HOST);
}

#[test]
fn set_changes_this_thread_only_until_set_again() {
    std::thread::spawn(|| {
        for platform in [Platform::MacOs, Platform::Linux, Platform::Windows] {
            set(platform);
            assert_eq!(current(), platform);
        }
    })
    .join()
    .unwrap();
}

#[test]
fn each_system_names_its_file_manager() {
    assert_eq!(Platform::MacOs.show_in_folder_label(), "Show in Finder");
    assert_eq!(Platform::Windows.show_in_folder_label(), "Show in Explorer");
    assert_eq!(Platform::Linux.show_in_folder_label(), "Show in folder");
    assert!(Platform::MacOs.writes_key_signs() && !Platform::Linux.writes_key_signs());
    assert!(Platform::Windows.draws_own_title_bar() && !Platform::MacOs.draws_own_title_bar());
}

#[test]
fn with_puts_the_system_back_also_after_a_panic() {
    std::thread::spawn(|| {
        set(Platform::Linux);
        assert_eq!(with(Platform::MacOs, current), Platform::MacOs);
        assert_eq!(current(), Platform::Linux);
        let panicked = std::panic::catch_unwind(|| with(Platform::Windows, || panic!("a failing check")));
        assert!(panicked.is_err());
        assert_eq!(current(), Platform::Linux);
    })
    .join()
    .unwrap();
}
