// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A project's plug-ins started for an offline render.

use super::*;

/// A project's plug-ins started for an offline render (ADR 0006: offline renders run the same
/// plug-ins through the same engine, without a deadline). Keep it until the render is done:
/// dropping it ends the host processes.
pub struct OfflinePlugins {
    /// For the renderer.
    pub plugins: Plugins,
    controls: Vec<Plugin>,
    /// The audio sources of the ARA documents plug-ins were loaded with (ADR 0028).
    _ara_sources: Vec<(String, suhina_plugins::AudioSource)>,
    /// What the user should hear about: missing plug-ins, ones that could not start or did not
    /// take their state.
    pub notices: Vec<String>,
}

impl OfflinePlugins {
    /// No plug-ins: a render without a catalog passes plug-in devices through.
    pub fn none() -> Self {
        Self { plugins: Plugins::new(), controls: Vec::new(), _ara_sources: Vec::new(), notices: Vec::new() }
    }

    /// Plug-ins running for this render.
    pub fn running(&self) -> usize {
        self.controls.len()
    }

    /// The ids of the host processes the running plug-ins are in, in the order they started:
    /// consecutive plug-ins in one process share a round trip a block (ADR 0006, decision 2).
    pub fn process_ids(&self) -> Vec<u32> {
        self.controls.iter().map(Plugin::process_id).collect()
    }
}

/// Starts every plug-in device of `project` that is installed, at `sample_rate`, and waits for
/// each. A plug-in that edits clips is given its track's audio clips from `audio` (ADR 0028).
pub fn start_offline(project: &Project, audio: &AudioPool, env: &PluginEnv, sample_rate: u32) -> OfflinePlugins {
    let mut offline = OfflinePlugins::none();
    let mut missing = Vec::new();
    // The render's ARA plug-ins share documents as the playing ones do, so it sounds as heard;
    // a key of its own keeps them apart from the playing ones in the same host process.
    let ara_document = next_ara_document();
    for found in plugin_devices(project) {
        let Some(device) = project
            .channels
            .iter()
            .find(|c| c.id == found.channel)
            .and_then(|c| suhina_model::device_at(&c.devices, &found.path))
        else {
            continue;
        };
        let Some((matched, instrument)) = find(&env.catalog, device) else {
            missing.push(device.name.clone());
            continue;
        };
        let ara = found
            .track
            .filter(|_| matched.plugin.ara)
            .and_then(|t| crate::ara::Layout::of(project, audio, t).document(audio));
        let ara = ara.map(|(mut document, sources)| {
            offline._ara_sources.extend(sources);
            document.document.clone_from(&ara_document);
            document
        });
        let launch = Launch {
            host: env.host.clone(),
            plugin: matched.plugin.spec(),
            channels: 2,
            max_frames: MAX_BLOCK_FRAMES as u32,
            sample_rate: f64::from(sample_rate),
            timeout: START_TIMEOUT,
            group: (env.group)(matched.plugin),
            ara,
        };
        match start(launch, saved_state(device)) {
            Ok((control, audio, refused)) => {
                if let Some(why) = refused {
                    offline.notices.push(format!("{} did not take its saved settings ({why})", device.name));
                }
                let info = PluginSlotInfo { instrument, latency: control.latency() };
                let hosted = Hosted::new(audio);
                offline.plugins.insert(found.key, info, Box::new(hosted));
                offline.controls.push(control);
            }
            Err(e) => offline.notices.push(format!("{} could not start: {e}", device.name)),
        }
    }
    if !missing.is_empty() {
        offline.notices.push(format!("not installed, passed through: {}", missing.join(", ")));
    }
    offline
}
