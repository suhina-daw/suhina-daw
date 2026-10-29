// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`Player`]: the running engine a session plays through, and [`OutputHandle`], what keeps
//! its audio output open.

use suhina_engine::{Controller, LauncherMonitor, Plugins};
use suhina_model::Project;

use crate::{CommandError, LiveOutput, OutputInfo, ParamEdit};

/// What keeps a player's audio output open (a stream, the thread that owns it): closed when it is
/// dropped, or by [`OutputHandle::close`] when the output layer opens another output and wants the
/// running plug-ins back.
pub trait OutputHandle: Send {
    /// Closes the output and gives back its sample rate and the plug-ins it ran, when they come
    /// back in time.
    fn close(self: Box<Self>) -> Option<(u32, Plugins)>;
}

/// A running audio engine that plays the session's project: transport and launcher commands
/// go to it. Whatever keeps the audio output open (a stream, a thread) is held in `keep_alive`
/// and closed when the player is dropped, or handed back with [`Player::into_parts`].
pub struct Player {
    controller: Controller,
    keep_alive: Option<Box<dyn OutputHandle>>,
    output: Option<std::sync::Arc<dyn LiveOutput>>,
    /// The launch quantisation last sent, which a new output is given again.
    quantization: Option<suhina_engine::Quantization>,
}

/// `controller`, with the follow actions' random numbers seeded from the clock, so each
/// playback picks its own (offline renders keep the engine's fixed seed).
fn seeded(mut controller: Controller) -> Controller {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    // A full queue means the engine is not running yet; it then keeps its fixed seed.
    let _ = controller.send(suhina_engine::LaunchCommand::SeedRandom(nanos as u64));
    controller
}

impl Player {
    pub fn new(controller: Controller, keep_alive: Option<Box<dyn OutputHandle>>) -> Self {
        Self { controller: seeded(controller), keep_alive, output: None, quantization: None }
    }

    /// A player whose output reports what it runs with (`audio.info`).
    pub fn with_output(
        controller: Controller,
        keep_alive: Option<Box<dyn OutputHandle>>,
        output: std::sync::Arc<dyn LiveOutput>,
    ) -> Self {
        Self { controller: seeded(controller), keep_alive, output: Some(output), quantization: None }
    }

    /// What the output runs with now, when it says.
    pub fn output_info(&self) -> Option<OutputInfo> {
        self.output.as_ref().map(|o| o.info())
    }

    /// The controller and whatever keeps the output open, for the output layer to close it in
    /// its own way (taking its plug-ins back) before opening another.
    pub fn into_parts(self) -> (Controller, Option<Box<dyn OutputHandle>>) {
        (self.controller, self.keep_alive)
    }

    pub fn send(&mut self, command: suhina_engine::LaunchCommand) -> Result<(), CommandError> {
        self.controller.send(command).map_err(CommandError)?;
        if let suhina_engine::LaunchCommand::SetQuantization(q) = command {
            self.quantization = Some(q);
        }
        Ok(())
    }

    /// The launch quantisation last sent to this player, if any.
    pub fn quantization(&self) -> Option<suhina_engine::Quantization> {
        self.quantization
    }

    pub fn monitor(&self) -> &LauncherMonitor {
        self.controller.monitor()
    }

    /// Makes the engine play `project` from now on, keeping what is playing (ADR 0005).
    pub fn update(&mut self, project: &Project) -> Result<(), CommandError> {
        let began = std::time::Instant::now();
        let updated = self.controller.update(project).map_err(CommandError);
        trace_update(began);
        updated
    }

    /// Like [`Player::update`], for a project with one scene inserted or removed since the last
    /// update: what plays and what is queued move with their slots.
    pub fn update_with_scenes(
        &mut self,
        project: &Project,
        scenes: Option<suhina_engine::SceneShift>,
    ) -> Result<(), CommandError> {
        let began = std::time::Instant::now();
        let updated = self.controller.update_with_scenes(project, scenes).map_err(CommandError);
        trace_update(began);
        updated
    }

    /// Sets one parameter of a device on a track from the next audio block, without building a
    /// new graph (ADR 0005, amendment).
    pub fn set_device_param(&mut self, change: ParamEdit) -> Result<(), CommandError> {
        self.controller.set_device_param(change.track, change.device, change.param, change.value).map_err(CommandError)
    }

    /// Builds the engine's graphs from now on with the project's file-backed devices' parsed
    /// models and IRs ([docs/spec/nam-amp-and-cab.md]).
    pub fn set_device_assets(&mut self, assets: std::sync::Arc<suhina_engine::DeviceAssets>) {
        self.controller.set_assets(assets);
    }

    /// The engine's sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.controller.sample_rate()
    }

    /// What graphs built from now on know about the project's plug-in devices (ADR 0006).
    pub fn set_plugin_info(&mut self, info: std::collections::HashMap<u64, suhina_engine::PluginSlotInfo>) {
        self.controller.set_plugin_info(info);
    }

    /// Builds the engine's graphs from now on with `audio` (ADR 0011).
    pub fn set_audio(&mut self, audio: std::sync::Arc<suhina_engine::AudioPool>) {
        self.controller.set_audio(audio);
    }

    /// Whether a preview plays, and where (ADR 0011).
    pub fn preview_monitor(&self) -> &suhina_engine::PreviewMonitor {
        self.controller.preview_monitor()
    }

    /// The audio thread's CPU load: the smoothed share of wall time it spends rendering,
    /// in per mille (1000 is busy all of the time).
    pub fn cpu_monitor(&self) -> &suhina_engine::CpuMonitor {
        self.controller.cpu_monitor()
    }

    /// Frames of stretched (warped) clips that were not ready in time and played as silence,
    /// so far.
    pub fn stretch_underruns(&self) -> u64 {
        self.controller.stretch_underruns()
    }

    /// Edits while playing whose new graph had to restart the stretched clips, so far.
    pub fn stretch_moves(&self) -> u64 {
        self.controller.stretch_moves()
    }

    /// Plug-in blocks not answered in time, and the plug-in (its key) that missed most.
    pub fn plugin_misses(&self) -> (u64, u64, u64) {
        self.controller.plugin_misses()
    }

    /// The level meters of the main output and every track.
    pub fn meter_monitor(&self) -> &suhina_engine::MeterMonitor {
        self.controller.meter_monitor()
    }

    /// The level meters' monitor by its own arc, for a front end's frame data (the EQ panel's
    /// analyser, docs/spec/eq.md › The panel › The analyser).
    pub fn meter_monitor_arc(&self) -> std::sync::Arc<suhina_engine::MeterMonitor> {
        self.controller.meter_monitor_arc()
    }

    pub(crate) fn controller_mut(&mut self) -> &mut Controller {
        &mut self.controller
    }

    /// The sending end of the engine's live-note queue (ADR 0017); `None` once taken.
    pub fn take_live_input(&mut self) -> Option<suhina_engine::LiveSender> {
        self.controller.take_live_input()
    }

    /// The reading end of what the engine records (`docs/spec/recording.md`); `None` once taken.
    pub fn take_recorder(&mut self) -> Option<suhina_engine::RecordReceiver> {
        self.controller.take_recorder()
    }

    /// Hands a running plug-in's audio half to the engine for the plug-in device `key`.
    pub fn send_plugin(
        &mut self,
        key: u64,
        processor: Box<dyn suhina_engine::PluginProcessor>,
    ) -> Result<(), CommandError> {
        self.controller.send_plugin(key, processor).map_err(CommandError)
    }
}

/// Traces an engine update that took the caller's thread [`suhina_plugins::trace::SLOW_WORK`] or
/// more, with what its graph's build took part by part.
fn trace_update(began: std::time::Instant) {
    let took = began.elapsed();
    if took >= suhina_plugins::trace::SLOW_WORK {
        suhina_plugins::trace!(
            "slow engine update: {:.1} ms ({})",
            took.as_secs_f64() * 1000.0,
            suhina_engine::build_report::last()
        );
    }
}
