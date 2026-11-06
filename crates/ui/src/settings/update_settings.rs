// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings › Agents and updates › Updates, in lines of their own in the settings file
//! (`docs/spec/updates.md` › Settings).

use suhina_update::{BuildKind, Channel};

/// The settings this module reads and writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateSettings {
    /// Asks GitHub once per start whether a newer build is out.
    pub check: bool,
    pub channel: Channel,
    /// Lists test builds as a channel.
    pub developer: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { check: true, channel: Channel::Nightly, developer: false }
    }
}

impl UpdateSettings {
    /// Whether the channel list has Test builds: in developer mode, or when the running build is
    /// itself a test build.
    pub fn test_builds_listed(&self) -> bool {
        self.developer || matches!(BuildKind::of(env!("CARGO_PKG_VERSION")), BuildKind::TestBuild { .. })
    }

    /// The channel updates come from: Test builds only while they are listed.
    pub fn channel(&self) -> Channel {
        if self.test_builds_listed() { self.channel } else { Channel::Nightly }
    }

    /// The lines this module adds to the settings file.
    pub fn to_text(&self) -> String {
        let on = |b: bool| if b { "on" } else { "off" };
        let channel = match self.channel {
            Channel::Nightly => "nightly",
            Channel::TestBuilds => "test_builds",
        };
        format!(
            "check_updates = {}\nupdate_channel = {channel}\ndeveloper_mode = {}\n",
            on(self.check),
            on(self.developer)
        )
    }

    /// Reads one line of the settings file; `false` when the key and value are not one of these
    /// settings, so the caller goes on with its own.
    pub fn parse(&mut self, key: &str, value: &str) -> bool {
        match (key, value) {
            ("check_updates", "on" | "off") => self.check = value == "on",
            ("update_channel", "nightly") => self.channel = Channel::Nightly,
            ("update_channel", "test_builds") => self.channel = Channel::TestBuilds,
            ("developer_mode", "on" | "off") => self.developer = value == "on",
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_read_back_what_they_write_and_check_by_default() {
        assert!(UpdateSettings::default().check);
        let settings = UpdateSettings { check: false, channel: Channel::TestBuilds, developer: true };
        let mut read = UpdateSettings::default();
        for line in settings.to_text().lines() {
            let (key, value) = line.split_once(" = ").unwrap();
            assert!(read.parse(key, value), "{line}");
        }
        assert_eq!(read, settings);
        assert!(!read.parse("update_channel", "beta"));
    }

    #[test]
    fn test_builds_are_the_channel_only_in_developer_mode() {
        let settings = UpdateSettings { check: true, channel: Channel::TestBuilds, developer: false };
        // A local build running these tests is not a test build.
        assert_eq!(settings.channel(), Channel::Nightly);
        assert_eq!(UpdateSettings { developer: true, ..settings }.channel(), Channel::TestBuilds);
    }
}
