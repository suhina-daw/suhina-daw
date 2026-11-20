// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{ExportEntry, Parameters, state};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamRescanFlags, PluginParams,
};
use clack_extensions::state::PluginState;
use clack_host::prelude::*;
use clack_host::utils::ClapId;
use std::cell::Cell;
use std::rc::Rc;

struct Host;
struct Shared;
struct Editor {
    notifications: Cell<usize>,
    flags: Cell<ParamRescanFlags>,
    cached_value: Cell<f64>,
}
struct Main(Rc<Editor>);
impl HostHandlers for Host {
    type Shared<'a> = Shared;
    type MainThread<'a> = Main;
    type AudioProcessor<'a> = ();
    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Shared) {
        builder.register::<HostParams>();
    }
}
impl SharedHandler<'_> for Shared {
    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {}
}
impl HostParamsImplShared for Shared {
    fn request_flush(&self) {}
}
impl MainThreadHandler<'_> for Main {}
impl HostParamsImplMainThread for Main {
    fn rescan(&self, flags: ParamRescanFlags) {
        self.0.notifications.set(self.0.notifications.get() + 1);
        self.0.flags.set(flags);
    }
    fn clear(&self, _id: ClapId, _flags: ParamClearFlags) {}
}

#[test]
fn successful_state_load_invalidates_cached_editor_values_but_rejected_state_does_not() {
    let editor = Rc::new(Editor {
        notifications: Cell::new(0),
        flags: Cell::new(ParamRescanFlags::empty()),
        cached_value: Cell::new(0.0),
    });
    let entry = PluginEntry::load_from_clack::<ExportEntry>(c"/native-state-test.clap").unwrap();
    let host = HostInfo::new("Suhina test host", "Suhina", "", "1").unwrap();
    let mut plugin = PluginInstance::<Host>::new(
        |_| Shared,
        |_| Main(Rc::clone(&editor)),
        &entry,
        c"org.suhina.device.utility",
        &host,
    )
    .unwrap();
    let handle = plugin.plugin_handle();
    let params = handle.get_extension::<PluginParams>().unwrap();
    let state_extension = handle.get_extension::<PluginState>().unwrap();
    let info = suhina_device_registry::DEVICES.iter().find(|info| info.id == "utility").unwrap();
    let parameters = Parameters::new(info).unwrap();
    let id = ClapId::new(parameters.id(0).unwrap());
    editor.cached_value.set(params.get_value(&handle, id).unwrap());
    parameters.control_set(id.get(), -12.0);
    let saved = state::save(&parameters);
    state_extension.load(&handle, &mut saved.as_slice()).unwrap();
    assert_eq!(editor.notifications.get(), 1);
    assert_eq!(editor.flags.get(), ParamRescanFlags::VALUES);
    assert_ne!(editor.cached_value.get(), -12.0, "the fixture caches values until the host handles rescan");
    if editor.flags.replace(ParamRescanFlags::empty()).contains(ParamRescanFlags::VALUES) {
        editor.cached_value.set(params.get_value(&handle, id).unwrap());
    }
    assert_eq!(editor.cached_value.get(), -12.0);
    let mut round_trip = Vec::new();
    state_extension.save(&handle, &mut round_trip).unwrap();
    assert_eq!(round_trip, saved, "no audio processing is needed to expose accepted state");
    assert!(state_extension.load(&handle, &mut &saved[..saved.len() - 1]).is_err());
    assert_eq!(editor.notifications.get(), 1);
    assert!(editor.flags.get().is_empty());
    assert_eq!(params.get_value(&handle, id), Some(editor.cached_value.get()));
}
