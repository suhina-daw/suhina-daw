// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! VST3 library entry and one processor/controller class pair per eligible native device.
use super::controller::Controller;
use super::*;

struct Factory {
    devices: Vec<&'static DeviceInfo>,
}
impl Class for Factory {
    type Interfaces = (IPluginFactory, IPluginFactory2);
}
impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        // SAFETY: the host supplies a writable factory description or null.
        let Some(info) = (unsafe { info.as_mut() }) else { return kInvalidArgument };
        text("Suhina", &mut info.vendor);
        text("", &mut info.url);
        text("", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }
    unsafe fn countClasses(&self) -> i32 {
        (self.devices.len() * 2) as i32
    }
    unsafe fn getClassInfo(&self, index: i32, output: *mut PClassInfo) -> tresult {
        let Some((info, controller)) = self.device(index) else { return kInvalidArgument };
        // SAFETY: the host supplies a writable class description or null.
        let Some(output) = (unsafe { output.as_mut() }) else { return kInvalidArgument };
        output.cid = class_id(&info.id, controller);
        output.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        text(if controller { "Component Controller Class" } else { "Audio Module Class" }, &mut output.category);
        text(&format!("Suhina {}", info.name), &mut output.name);
        kResultOk
    }
    unsafe fn createInstance(&self, cid: FIDString, iid: FIDString, object: *mut *mut c_void) -> tresult {
        if cid.is_null() || iid.is_null() || object.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host provides exactly sixteen class-id bytes; no alignment is assumed.
        let cid = unsafe { std::ptr::read_unaligned(cid.cast::<TUID>()) };
        // SAFETY: object points to one writable output pointer.
        unsafe { object.write(std::ptr::null_mut()) };
        for &info in &self.devices {
            let instance = if cid == class_id(&info.id, false) {
                let Ok(parameters) = Parameters::new(info) else { return kResultFalse };
                ComWrapper::new(Component { parameters, runtime: RefCell::new(None), latency: AtomicU32::new(0) })
                    .to_com_ptr::<FUnknown>()
            } else if cid == class_id(&info.id, true) {
                let Ok(parameters) = Parameters::new(info) else { return kResultFalse };
                ComWrapper::new(Controller { parameters }).to_com_ptr::<FUnknown>()
            } else {
                continue;
            };
            let Some(instance) = instance else { return kResultFalse };
            let pointer = instance.as_ptr();
            // SAFETY: instance is a live COM object, so its vtable is live.
            let table = unsafe { (*pointer).vtbl };
            // SAFETY: live vtable contains queryInterface.
            let query = unsafe { (*table).queryInterface };
            // SAFETY: query is this object's method and host iid/output are valid for the call.
            return unsafe { query(pointer, iid.cast_mut().cast(), object) };
        }
        kInvalidArgument
    }
}
impl Factory {
    fn device(&self, index: i32) -> Option<(&'static DeviceInfo, bool)> {
        let index = usize::try_from(index).ok()?;
        self.devices.get(index / 2).copied().map(|info| (info, !index.is_multiple_of(2)))
    }
}
impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, output: *mut PClassInfo2) -> tresult {
        let Some((info, controller)) = self.device(index) else { return kInvalidArgument };
        // SAFETY: host supplies writable class metadata or null.
        let Some(output) = (unsafe { output.as_mut() }) else { return kInvalidArgument };
        output.cid = class_id(&info.id, controller);
        output.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        output.classFlags = 0;
        text(if controller { "Component Controller Class" } else { "Audio Module Class" }, &mut output.category);
        text(&format!("Suhina {}", info.name), &mut output.name);
        text(if info.role == Role::Instrument { "Instrument|Synth" } else { "Fx" }, &mut output.subCategories);
        text("Suhina", &mut output.vendor);
        text(env!("CARGO_PKG_VERSION"), &mut output.version);
        text("VST 3.8", &mut output.sdkVersion);
        kResultOk
    }
}

// SAFETY: these are the VST3 module's prescribed exported symbol names/signatures.
#[unsafe(no_mangle)]
extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    let devices =
        suhina_device_registry::DEVICES.iter().copied().filter(|info| super::super::exportable(info)).collect();
    ComWrapper::new(Factory { devices })
        .to_com_ptr::<IPluginFactory>()
        .map(|factory| factory.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
#[cfg(windows)]
#[unsafe(no_mangle)]
extern "system" fn InitDll() -> bool {
    true
}
#[cfg(windows)]
#[unsafe(no_mangle)]
extern "system" fn ExitDll() -> bool {
    true
}
#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleEntry(_handle: *mut c_void) -> bool {
    true
}
#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleExit() -> bool {
    true
}
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn bundleEntry(_handle: *mut c_void) -> bool {
    true
}
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn bundleExit() -> bool {
    true
}
