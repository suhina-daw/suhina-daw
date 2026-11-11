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
    /// The audio sources of the ARA documents plug-ins were loaded with (ADR 0033).
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

/// One plug-in device started as an offline render starts it ([`start_offline_device`]): its
/// control and audio halves, for a caller that runs its blocks itself (`suhina-cli plugin
/// run-device`). Keep it until the render is done: dropping it ends the host process.
pub struct OfflineDevice {
    pub control: Plugin,
    pub audio: PluginAudio,
    /// Whether the plug-in is an instrument.
    pub instrument: bool,
    /// Why the plug-in did not take the state the project saved, if it did not; it runs with
    /// its own defaults then.
    pub refused: Option<String>,
    /// The audio sources of the ARA document it was loaded with (ADR 0033).
    _ara_sources: Vec<(String, suhina_plugins::AudioSource)>,
}

/// Starts every plug-in device of `project` that is installed, at `sample_rate`, and waits for
/// each. A plug-in that edits clips is given its track's audio clips from `audio` (ADR 0033).
pub fn start_offline(project: &Project, audio: &AudioPool, env: &PluginEnv, sample_rate: u32) -> OfflinePlugins {
    let mut offline = OfflinePlugins::none();
    let mut missing = Vec::new();
    // The render's ARA plug-ins share documents as the playing ones do, so it sounds as heard;
    // a key of its own keeps them apart from the playing ones in the same host process.
    let ara_document = next_ara_document();
    for found in plugin_devices(project) {
        let Some(device) = device_of(project, &found) else { continue };
        match start_one(project, audio, env, &found, sample_rate, &ara_document) {
            Ok(Some(mut started)) => {
                if let Some(why) = started.refused.take() {
                    offline.notices.push(format!("{} did not take its saved settings ({why})", device.name));
                }
                let info = PluginSlotInfo { instrument: started.instrument, latency: started.control.latency() };
                // Its stepped parameters' lanes hold their values, as they do while playing.
                if let Ok(list) = started.control.params() {
                    offline.plugins.set_steps(found.key, super::steps(&list));
                }
                let hosted = Hosted::new(started.audio).with_instrument_transport(started.instrument, sample_rate);
                offline.plugins.insert(found.key, info, Box::new(hosted));
                offline.controls.push(started.control);
                offline._ara_sources.extend(started._ara_sources);
            }
            Ok(None) => missing.push(device.name.clone()),
            Err(e) => offline.notices.push(format!("{} could not start: {e}", device.name)),
        }
    }
    if !missing.is_empty() {
        offline.notices.push(format!("not installed, passed through: {}", missing.join(", ")));
    }
    offline
}

/// Starts the one plug-in device `key` of `project` as [`start_offline`] starts each: the same
/// installed plug-in, saved state, process group and clips (ADR 0033). `Ok(None)` when no
/// installed plug-in stands for it, or `key` is no plug-in device of `project`.
pub fn start_offline_device(
    project: &Project,
    audio: &AudioPool,
    env: &PluginEnv,
    key: u64,
    sample_rate: u32,
) -> Result<Option<OfflineDevice>, PluginError> {
    let Some(found) = plugin_devices(project).into_iter().find(|d| d.key == key) else { return Ok(None) };
    start_one(project, audio, env, &found, sample_rate, &next_ara_document())
}

fn device_of<'a>(project: &'a Project, found: &suhina_engine::PluginDevice) -> Option<&'a Device> {
    project
        .channels
        .iter()
        .find(|c| c.id == found.channel)
        .and_then(|c| suhina_model::device_at(&c.devices, &found.path))
}

/// Starts the plug-in device `found` for an offline render, in the ARA document
/// `ara_document`; `Ok(None)` when it is not installed.
fn start_one(
    project: &Project,
    audio: &AudioPool,
    env: &PluginEnv,
    found: &suhina_engine::PluginDevice,
    sample_rate: u32,
    ara_document: &str,
) -> Result<Option<OfflineDevice>, PluginError> {
    let Some(device) = device_of(project, found) else { return Ok(None) };
    let Some((matched, instrument)) = find(&env.catalog, device) else { return Ok(None) };
    let mut ara_sources = Vec::new();
    let ara = found
        .track
        .filter(|_| matched.plugin.ara)
        .and_then(|t| crate::ara::Layout::of(project, audio, t).document(audio));
    let ara = ara.map(|(mut document, sources)| {
        ara_sources = sources;
        ara_document.clone_into(&mut document.document);
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
    let (control, audio, refused) = start(launch, saved_state(device))?;
    Ok(Some(OfflineDevice { control, audio, instrument, refused, _ara_sources: ara_sources }))
}
