// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::settings::Settings;

/// Kept in `settings.json` but changed outside the Settings window: the browser and the map
/// window as they were left.
const NOT_IN_THE_WINDOW: [&str; 8] = [
    "setting_descriptions",
    "browser",
    "browser_width",
    "map_dot_size",
    "map_window",
    "map_window_rect",
    "session_mixer_height",
    "session_strip_fader",
];

fn saved_keys() -> Vec<String> {
    let settings = Settings { midi_inputs_off: vec!["Keys".into()], ..Settings::default() };
    settings
        .to_text()
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once('=').map(|(key, _)| key.trim().to_string()))
        .collect()
}

#[test]
fn every_saved_setting_is_on_exactly_one_page() {
    for key in saved_keys() {
        let on: Vec<&str> = ROWS.iter().filter(|e| e.keys.contains(&key.as_str())).map(|e| e.name).collect();
        if NOT_IN_THE_WINDOW.contains(&key.as_str()) {
            assert!(on.is_empty(), "{key} is changed outside the window, yet shown by {on:?}");
        } else {
            assert_eq!(on.len(), 1, "{key} is shown by {on:?}");
        }
    }
    // And every key an entry names is one the settings file has, or the keymap.
    let saved = saved_keys();
    for entry in ROWS {
        for key in entry.keys {
            assert!(saved.iter().any(|k| k == key) || KEYMAP.contains(key), "{}: no saved key {key}", entry.name);
        }
    }
}

#[test]
fn every_entry_has_a_name_of_its_own_and_its_sections_stay_together() {
    let mut names: Vec<&str> = ROWS.iter().map(|e| e.name).collect();
    names.sort_unstable();
    let count = names.len();
    names.dedup();
    assert_eq!(names.len(), count, "two entries share a name");
    // A page's sections and the pages themselves come in one run each, in the list's order.
    let mut seen: Vec<(Page, &str)> = Vec::new();
    for entry in ROWS {
        let here = (entry.page, entry.section);
        if seen.last() != Some(&here) {
            assert!(!seen.contains(&here), "{} › {} is split in two", entry.page.name(), entry.section);
            seen.push(here);
        }
    }
    let order: Vec<Page> = Page::ALL.iter().map(|(page, _)| *page).collect();
    let mut pages: Vec<Page> = seen.iter().map(|(page, _)| *page).collect();
    pages.dedup();
    assert_eq!(pages, order, "every page has settings, in the order of the list");
}

const RULES: &str = "docs/spec/ui-settings.md > Adding a setting";

/// Words a newcomer cannot read, kept out of descriptions (docs/spec/ui-settings.md › Adding a
/// setting, rule 4); a setting's options or help may name them when someone needs them.
const JARGON: [&str; 7] = ["underruns", "underrun", "MCP", "IR", "latency", "ADR", "buffer"];

#[test]
fn every_entry_has_a_page_a_group_and_a_one_sentence_description() {
    for entry in ROWS {
        assert!(!entry.name.trim().is_empty(), "an entry has no name ({RULES})");
        assert!(!entry.section.trim().is_empty(), "{}: no group ({RULES})", entry.name);
        assert!(!entry.note.trim().is_empty(), "{}: no description ({RULES})", entry.name);
        let developer = ["docs/", ".md", "[`", "ADR "].iter().any(|word| entry.note.contains(word));
        assert!(
            !developer,
            "{}: a description is for the person using Suhina, not a link to the docs ({RULES})",
            entry.name
        );
        let sentences = [". ", "? ", "! "].iter().any(|end| entry.note.contains(end)) || entry.note.ends_with('.');
        assert!(!sentences, "{}: the description is more than one sentence ({RULES})", entry.name);
        // One thought: no second clause after a semicolon, such as what the off state does.
        assert!(!entry.note.contains(';'), "{}: the description holds two thoughts ({RULES})", entry.name);
        // A name says what the setting does in one to four words.
        let words = entry.name.split_whitespace().count();
        assert!(words <= 4, "{}: the name has {words} words, at most four ({RULES})", entry.name);
        // A name reads on its own, not as a sentence cut off before its buttons.
        let cut = [" by", " at", " to", " is"].iter().any(|end| entry.name.ends_with(end));
        assert!(!cut, "{}: the name is a sentence cut off before its buttons ({RULES})", entry.name);
        // The switch shows the off state, and the Keys page lists shortcuts.
        let note = entry.note.to_lowercase();
        assert!(!note.contains("off,"), "{}: the description explains the off state ({RULES})", entry.name);
        let shortcut = ["Ctrl+", "Alt+", "Shift+", "Cmd+"].iter().any(|key| entry.note.contains(key));
        assert!(!shortcut, "{}: the description names a shortcut ({RULES})", entry.name);
        // Plain words: the effect, not the mechanism.
        for word in JARGON {
            let found = format!(" {note} ").contains(&format!(" {} ", word.to_lowercase()));
            assert!(!found, "{}: the description uses {word:?}; say what it does instead ({RULES})", entry.name);
        }
    }
}

#[test]
fn a_group_outside_advanced_has_at_most_seven_entries() {
    for entry in ROWS.iter().filter(|e| e.section != ADVANCED) {
        let count = ROWS.iter().filter(|e| e.page == entry.page && e.section == entry.section).count();
        assert!(
            count <= 7,
            "{} > {} has {count} entries; split it or move the rare ones under {ADVANCED} ({RULES})",
            entry.page.name(),
            entry.section
        );
    }
}

#[test]
fn advanced_is_last_on_its_page_and_status_and_commands_are_set_off_at_the_top() {
    for (page, _) in Page::ALL {
        let on_page: Vec<&Entry> = ROWS.iter().filter(|e| e.page == page).collect();
        if let Some(first) = on_page.iter().position(|e| e.section == ADVANCED) {
            assert!(
                on_page[first..].iter().all(|e| e.section == ADVANCED),
                "{}: {ADVANCED} is not last ({RULES})",
                page.name()
            );
        }
        let mut seen_setting = false;
        for entry in on_page {
            if entry.role == Role::Setting {
                seen_setting = true;
            } else {
                assert!(
                    !seen_setting,
                    "{}: {} is a status or a command among settings; put it first in its own group ({RULES})",
                    page.name(),
                    entry.name
                );
            }
        }
    }
}

#[test]
fn a_search_finds_a_setting_by_a_word_of_its_description_or_its_group() {
    let by = |word: &str| -> Vec<&str> {
        let words = vec![word.to_lowercase()];
        ROWS.iter().filter(|e| e.matches_words(&words)).map(|e| e.name).collect()
    };
    assert!(by("headphones").contains(&"Headphone output"), "a word of the description");
    assert!(by("previews").contains(&"Preview selected file"), "the group's name");
}
