// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`HostedPlugin`]: what the host process does with a loaded plug-in, whatever its format
//! (#764 › 10). CLAP, VST3 and the test processors each implement it; what a format lacks (an
//! editor, ARA) has a default that says so, so the host's loop needs no match on the format.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::Instant;

use suhina_plugins::ParamInfo;
use suhina_plugins::protocol::{AraDocument, AraNote};

use crate::processors::Processor;
use crate::{SETTLE, SETTLING, UNLOAD_WAIT, audio_stopped, clap, vst3, window};

/// A plug-in instance the process runs.
pub(crate) type Loaded = Box<dyn HostedPlugin>;

const NOT_ARA: &str = "only a VST3 plug-in can be bound to an ARA document";

/// A loaded plug-in, as the host's loop drives it.
pub(crate) trait HostedPlugin {
    fn latency(&self) -> u32;

    /// A latency the plug-in reported since this was last asked, after restarting its
    /// processing where its format asks for that: the engine is to be told.
    fn latency_changed(&mut self) -> Option<u32>;

    /// Prepares it again for `sample_rate`; its latency then.
    fn reconfigure(&mut self, _sample_rate: f64) -> Result<u32, String> {
        Err("this test processor cannot reconfigure".into())
    }

    fn open_editor(&mut self, _mode: window::Mode, _title: Option<&str>) -> Result<window::Size, String> {
        Err("the plug-in has no editor".into())
    }

    /// Sets what the title bar of the editor's window says.
    fn set_editor_title(&self, _title: &str) {}

    /// Puts an embedded editor where the engine says.
    fn place_editor(&self, _placement: window::Placement) {}

    /// Fits an open editor to the size the engine gives it.
    fn fit_editor(&mut self, _size: window::Size) {}

    /// A size the open editor got since this was last asked, to tell the engine.
    fn editor_resized(&mut self) -> Option<window::Size> {
        None
    }

    fn close_editor(&mut self) {}

    /// Keeps an open editor in step with its window; whether the user closed it.
    fn editor_tick(&mut self) -> bool {
        false
    }

    fn params(&mut self) -> Result<Vec<ParamInfo>, String> {
        Ok(Vec::new())
    }

    fn set_param(&mut self, id: u32, _value: f64) -> Result<ParamInfo, String> {
        Err(format!("no parameter {id}"))
    }

    /// The component's state and, for VST3, the controller's.
    fn state(&mut self) -> Result<(Vec<u8>, Option<Vec<u8>>), String>;

    fn set_state(&mut self, data: &[u8], controller: Option<&[u8]>) -> Result<(), String>;

    /// The parameters the plug-in changed itself since the last call.
    fn tick(&mut self) -> Vec<(u32, f64)> {
        Vec::new()
    }

    /// Its ARA side, when it edits clips (ADR 0033).
    fn ara(&self) -> Option<&dyn AraHost> {
        None
    }

    fn ara_mut(&mut self) -> Option<&mut dyn AraHost> {
        None
    }

    /// Closes the editor, stops the audio thread and lets go of the plug-in. An audio thread
    /// that does not stop is stuck in the plug-in's code: the plug-in is then left as it is
    /// (never deactivated or freed under it), so the other instances go on.
    fn unload(self: Box<Self>);
}

/// A plug-in bound to an ARA document: it edits the audio clips of its track (ADR 0033).
pub(crate) trait AraHost {
    /// Whether the plug-in supports ARA.
    fn edits_clips(&self) -> bool;
    /// How far its analysis has come since the last call, if it moved.
    fn take_ara_analysis(&self) -> Option<Option<f32>>;
    /// The notes it found in its regions since the last call (#720).
    fn take_ara_notes(&self) -> Vec<(String, Vec<AraNote>, bool)>;
    /// Brings its ARA document in line with the track's clips.
    fn ara_update(&mut self, document: &AraDocument) -> Result<(), String>;
    /// The edits of one modification and its source, stored on their own (ADR 0033 step 6).
    fn ara_export(&self, modification: &str, source: &str) -> Result<(String, Vec<u8>), String>;
    /// Gives it edits another track's stored, for one of its modifications.
    fn ara_import(&self, id: &str, bytes: &[u8], from: &str, to: &str, source: &str) -> Result<(), String>;
    /// Tells it which region the engine has selected.
    fn ara_select(&self, region: Option<&str>);
}

/// What the host's loop asks of any plug-in's ARA side: a plug-in without one answers as such.
impl dyn HostedPlugin {
    pub(crate) fn edits_clips(&self) -> bool {
        self.ara().is_some_and(AraHost::edits_clips)
    }

    pub(crate) fn take_ara_analysis(&self) -> Option<Option<f32>> {
        self.ara()?.take_ara_analysis()
    }

    pub(crate) fn take_ara_notes(&self) -> Vec<(String, Vec<AraNote>, bool)> {
        self.ara().map(AraHost::take_ara_notes).unwrap_or_default()
    }

    pub(crate) fn ara_update(&mut self, document: &AraDocument) -> Result<(), String> {
        self.ara_mut().ok_or(NOT_ARA)?.ara_update(document)
    }

    pub(crate) fn ara_export(&self, modification: &str, source: &str) -> Result<(String, Vec<u8>), String> {
        self.ara().ok_or(NOT_ARA)?.ara_export(modification, source)
    }

    pub(crate) fn ara_import(&self, id: &str, bytes: &[u8], from: &str, to: &str, source: &str) -> Result<(), String> {
        self.ara().ok_or(NOT_ARA)?.ara_import(id, bytes, from, to, source)
    }

    pub(crate) fn ara_select(&self, region: Option<&str>) {
        if let Some(ara) = self.ara() {
            ara.ara_select(region);
        }
    }
}

impl HostedPlugin for clap::ClapPlugin {
    fn latency(&self) -> u32 {
        clap::ClapPlugin::latency(self)
    }
    fn latency_changed(&mut self) -> Option<u32> {
        clap::ClapPlugin::latency_changed(self)
    }
    fn reconfigure(&mut self, sample_rate: f64) -> Result<u32, String> {
        clap::ClapPlugin::reconfigure(self, sample_rate)
    }
    fn open_editor(&mut self, mode: window::Mode, title: Option<&str>) -> Result<window::Size, String> {
        clap::ClapPlugin::open_editor(self, mode, title)
    }
    fn set_editor_title(&self, title: &str) {
        clap::ClapPlugin::set_editor_title(self, title);
    }
    fn place_editor(&self, placement: window::Placement) {
        clap::ClapPlugin::place_editor(self, placement);
    }
    fn fit_editor(&mut self, size: window::Size) {
        clap::ClapPlugin::fit_editor(self, size);
    }
    fn editor_resized(&mut self) -> Option<window::Size> {
        clap::ClapPlugin::editor_resized(self)
    }
    fn close_editor(&mut self) {
        clap::ClapPlugin::close_editor(self);
    }
    fn editor_tick(&mut self) -> bool {
        clap::ClapPlugin::editor_tick(self)
    }
    fn params(&mut self) -> Result<Vec<ParamInfo>, String> {
        Ok(clap::ClapPlugin::params(self))
    }
    fn set_param(&mut self, id: u32, value: f64) -> Result<ParamInfo, String> {
        clap::ClapPlugin::set_param(self, id, value)
    }
    fn state(&mut self) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
        Ok((clap::ClapPlugin::state(self)?, None))
    }
    fn set_state(&mut self, data: &[u8], _controller: Option<&[u8]>) -> Result<(), String> {
        clap::ClapPlugin::set_state(self, data)
    }
    // What a CLAP plug-in changed itself comes out with the blocks as output parameter events,
    // which its audio thread hands over here; its listing still read its values again as they
    // stand.
    fn tick(&mut self) -> Vec<(u32, f64)> {
        clap::ClapPlugin::tick(self)
    }
    fn unload(mut self: Box<Self>) {
        self.close_editor();
        if self.stop_audio(UNLOAD_WAIT) {
            drop(self);
        } else {
            std::mem::forget(self);
        }
    }
}

impl HostedPlugin for vst3::Vst3Plugin {
    fn latency(&self) -> u32 {
        vst3::Vst3Plugin::latency(self)
    }
    fn latency_changed(&mut self) -> Option<u32> {
        vst3::Vst3Plugin::latency_changed(self)
    }
    fn reconfigure(&mut self, sample_rate: f64) -> Result<u32, String> {
        vst3::Vst3Plugin::reconfigure(self, sample_rate)
    }
    fn open_editor(&mut self, mode: window::Mode, title: Option<&str>) -> Result<window::Size, String> {
        vst3::Vst3Plugin::open_editor(self, mode, title)
    }
    fn set_editor_title(&self, title: &str) {
        vst3::Vst3Plugin::set_editor_title(self, title);
    }
    fn place_editor(&self, placement: window::Placement) {
        vst3::Vst3Plugin::place_editor(self, placement);
    }
    fn fit_editor(&mut self, size: window::Size) {
        vst3::Vst3Plugin::fit_editor(self, size);
    }
    fn editor_resized(&mut self) -> Option<window::Size> {
        vst3::Vst3Plugin::editor_resized(self)
    }
    fn close_editor(&mut self) {
        vst3::Vst3Plugin::close_editor(self);
    }
    fn editor_tick(&mut self) -> bool {
        vst3::Vst3Plugin::editor_tick(self)
    }
    fn params(&mut self) -> Result<Vec<ParamInfo>, String> {
        Ok(vst3::Vst3Plugin::params(self))
    }
    fn set_param(&mut self, id: u32, value: f64) -> Result<ParamInfo, String> {
        vst3::Vst3Plugin::set_param(self, id, value)
    }
    fn state(&mut self) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
        Ok((vst3::Vst3Plugin::state(self)?, self.controller_state()))
    }
    fn set_state(&mut self, data: &[u8], controller: Option<&[u8]>) -> Result<(), String> {
        vst3::Vst3Plugin::set_state(self, data, controller)
    }
    fn tick(&mut self) -> Vec<(u32, f64)> {
        vst3::Vst3Plugin::tick(self)
    }
    fn ara(&self) -> Option<&dyn AraHost> {
        Some(self)
    }
    fn ara_mut(&mut self) -> Option<&mut dyn AraHost> {
        Some(self)
    }
    fn unload(mut self: Box<Self>) {
        suhina_plugins::trace!("vst3 unload: stop audio");
        if self.stop_audio(UNLOAD_WAIT) {
            // The editor closes and the plug-in goes from the main loop, after the engine has its
            // answer: an editor embedded in the engine's window is a child of a window of the
            // engine's process, and taking it down sends that window messages the engine answers
            // only once it stops waiting for this unload (Melodyne held both for as long as the
            // engine waited, #816). One that has not settled since it loaded waits for that too.
            self.leave_document();
            let at = Instant::now() + SETTLE.saturating_sub(self.age());
            suhina_plugins::trace!("vst3 unload: audio stopped; editor closed and released from the main loop");
            SETTLING.with(|settling| settling.borrow_mut().push((at, *self)));
        } else {
            self.close_editor();
            std::mem::forget(self);
        }
    }
}

impl AraHost for vst3::Vst3Plugin {
    fn edits_clips(&self) -> bool {
        vst3::Vst3Plugin::edits_clips(self)
    }
    fn take_ara_analysis(&self) -> Option<Option<f32>> {
        vst3::Vst3Plugin::take_ara_analysis(self)
    }
    fn take_ara_notes(&self) -> Vec<(String, Vec<AraNote>, bool)> {
        vst3::Vst3Plugin::take_ara_notes(self)
    }
    fn ara_update(&mut self, document: &AraDocument) -> Result<(), String> {
        vst3::Vst3Plugin::ara_update(self, document)
    }
    fn ara_export(&self, modification: &str, source: &str) -> Result<(String, Vec<u8>), String> {
        vst3::Vst3Plugin::ara_export(self, modification, source)
    }
    fn ara_import(&self, id: &str, bytes: &[u8], from: &str, to: &str, source: &str) -> Result<(), String> {
        vst3::Vst3Plugin::ara_import(self, id, bytes, from, to, source)
    }
    fn ara_select(&self, region: Option<&str>) {
        vst3::Vst3Plugin::ara_select(self, region);
    }
}

/// A test processor served on an audio thread of its own; `reported` is the latency the
/// engine was last told.
pub(crate) struct TestPlugin {
    pub(crate) processor: Arc<dyn Processor>,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) audio: thread::JoinHandle<()>,
    pub(crate) reported: u32,
}

impl HostedPlugin for TestPlugin {
    fn latency(&self) -> u32 {
        self.processor.latency()
    }
    fn latency_changed(&mut self) -> Option<u32> {
        let now = self.processor.latency();
        (now != self.reported).then(|| {
            self.reported = now;
            now
        })
    }
    fn state(&mut self) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
        Ok((self.processor.state(), None))
    }
    fn set_state(&mut self, data: &[u8], _controller: Option<&[u8]>) -> Result<(), String> {
        self.processor.set_state(data)
    }
    fn unload(self: Box<Self>) {
        let Self { stop, audio, .. } = *self;
        if audio_stopped(&stop, Some(&audio), UNLOAD_WAIT) {
            let _ = audio.join();
        }
    }
}
