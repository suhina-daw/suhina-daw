// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the status bar says when one of Suhina's own files (settings, keys, themes, the recent
//! projects) cannot be written.

use std::io::{Error, ErrorKind};

/// "could not save `what`: …". A write the system denies is most often security software that
/// does not trust a new, unsigned program (#180): the message says so and what to do, and that
/// the change itself is not lost, since the window keeps it until it closes.
pub fn message(what: &str, error: &Error) -> String {
    if error.kind() == ErrorKind::PermissionDenied {
        format!(
            "could not save {what}: writing was denied. Security software may be blocking Suhina: allow \
             it there. The change is kept until Suhina closes."
        )
    } else {
        format!("could not save {what}: {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_denied_write_names_security_software_and_keeps_the_change() {
        let text = message("settings.txt", &Error::from(ErrorKind::PermissionDenied));
        assert!(text.starts_with("could not save settings.txt: writing was denied."));
        assert!(text.contains("Security software may be blocking Suhina"));
        assert!(text.contains("kept until Suhina closes"));
    }

    #[test]
    fn other_errors_are_said_as_they_are() {
        let text = message("keymap.json", &Error::other("the disk is full"));
        assert_eq!(text, "could not save keymap.json: the disk is full");
    }
}
