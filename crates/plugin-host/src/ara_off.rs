// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! ARA left out: the 32-bit Windows host (the bridge for 32-bit plug-ins). The ARA bindings
//! (`ara_sys`) are generated for 64-bit processors and check their layout as they compile, and
//! ARA plug-ins are 64-bit, so this host binds none: [`supported`] says no, [`Document`] has no
//! values, and a `load` with a document fails with a reason. States read and write as the
//! 64-bit host's do ([`crate::ara_state`]).

use suhina_plugins::protocol::{AraDocument, AraNote, AraPlayback};
use vst3::ComPtr;
use vst3::Steinberg::Vst::IComponent;

pub(crate) use crate::ara_state::{unwrap_state, wrap_state};

/// Why a document cannot be bound here.
const NO_ARA: &str = "ARA is not available in the 32-bit plug-in host";

/// Whether the component offers ARA: never here.
pub(crate) fn supported(_component: &ComPtr<IComponent>) -> bool {
    false
}

/// No transport requests: no plug-in here can make one.
pub(crate) fn take_playback_requests() -> Vec<AraPlayback> {
    Vec::new()
}

/// The engine's position as it is: no ARA plug-in here holds another.
pub(crate) fn shown_position(engine: i64, _sample_rate: f64, _playing: bool) -> i64 {
    engine
}

/// A document controller, which this host never has: the type has no values.
pub(crate) enum Document {}

/// What an edit changed, which no edit here makes.
#[derive(Debug, Default)]
pub(crate) struct Edited;

impl Edited {
    pub(crate) fn changes_renderers(&self) -> bool {
        false
    }
}

impl Document {
    pub(crate) fn bind(_component: &ComPtr<IComponent>, _document: &AraDocument) -> Result<Self, String> {
        Err(NO_ARA.into())
    }

    pub(crate) fn track(&self) -> &str {
        match *self {}
    }

    pub(crate) fn select(&self, _region: Option<&str>) -> bool {
        match *self {}
    }

    pub(crate) fn select_after(&self, _edited: &Edited) {
        match *self {}
    }

    pub(crate) fn view_placed(&self, _shown: bool) {
        match *self {}
    }

    pub(crate) fn tick(&self) {
        match *self {}
    }

    pub(crate) fn take_analysis(&self) -> Option<Option<f32>> {
        match *self {}
    }

    pub(crate) fn take_notes(&self) -> Vec<(String, Vec<AraNote>, bool)> {
        match *self {}
    }

    pub(crate) fn store(&self) -> Option<(String, Vec<u8>)> {
        match *self {}
    }

    pub(crate) fn export(&self, _modification: &str, _source: &str) -> Result<(String, Vec<u8>), String> {
        match *self {}
    }

    pub(crate) fn import(&self, _id: &str, _bytes: &[u8], _from: &str, _to: &str, _source: &str) -> Result<(), String> {
        match *self {}
    }

    pub(crate) fn restore(&self, _id: &str, _bytes: &[u8]) -> Result<(), String> {
        match *self {}
    }

    pub(crate) fn edit(&mut self, _document: &AraDocument) -> Result<Edited, String> {
        match *self {}
    }

    pub(crate) fn renderers(&self, _edited: &Edited) {
        match *self {}
    }

    pub(crate) fn destroy(&mut self, _edited: &Edited) {
        match *self {}
    }
}
