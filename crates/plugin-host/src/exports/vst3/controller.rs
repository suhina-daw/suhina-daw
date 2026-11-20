// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Host parameter controller; the custom SDK editor is separate follow-on work.
use super::*;

pub(super) struct Controller {
    pub(super) parameters: Parameters,
}
impl Class for Controller {
    type Interfaces = (IEditController,);
}
impl IPluginBaseTrait for Controller {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}
impl IEditControllerTrait for Controller {
    unsafe fn setComponentState(&self, stream: *mut IBStream) -> tresult {
        // SAFETY: host supplies the component state stream for this call.
        unsafe { read_state(&self.parameters, stream) }
    }
    unsafe fn setState(&self, stream: *mut IBStream) -> tresult {
        // SAFETY: host supplies the controller state stream for this call.
        unsafe { read_state(&self.parameters, stream) }
    }
    unsafe fn getState(&self, stream: *mut IBStream) -> tresult {
        // SAFETY: host supplies a writable state stream for this call.
        unsafe { write_state(&self.parameters, stream) }
    }
    unsafe fn getParameterCount(&self) -> i32 {
        self.parameters.info.params.len() as i32
    }
    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        let Some(parameter) = usize::try_from(index).ok().and_then(|index| self.parameters.info.params.get(index))
        else {
            return kInvalidArgument;
        };
        // SAFETY: host supplies one writable ParameterInfo or null.
        let Some(info) = (unsafe { info.as_mut() }) else { return kInvalidArgument };
        info.id = super::super::parameter_id(&parameter.id);
        wide(&parameter.name, &mut info.title);
        wide(&parameter.name, &mut info.shortTitle);
        wide("", &mut info.units);
        info.stepCount = match parameter.kind {
            ParamKind::Continuous(_) => 0,
            _ => (parameter.max - parameter.min).max(1.0) as i32,
        };
        info.defaultNormalizedValue = parameter.to_normalized(parameter.default);
        info.unitId = 0;
        info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate as int32;
        kResultOk
    }
    unsafe fn getParamStringByValue(&self, id: u32, value: f64, output: *mut String128) -> tresult {
        let Some(parameter) = self.parameters.index(id).and_then(|index| self.parameters.info.params.get(index)) else {
            return kInvalidArgument;
        };
        // SAFETY: the host supplies one writable 128-character array or null.
        let Some(output) = (unsafe { output.as_mut() }) else { return kInvalidArgument };
        let value = parameter.from_normalized(value);
        match &parameter.kind {
            ParamKind::Choice(choices) => wide(choices.get(value as usize).unwrap_or(""), output),
            _ => wide(&format!("{value:.3}"), output),
        }
        kResultOk
    }
    unsafe fn getParamValueByString(&self, id: u32, input: *mut TChar, value: *mut f64) -> tresult {
        let Some(parameter) = self.parameters.index(id).and_then(|index| self.parameters.info.params.get(index)) else {
            return kInvalidArgument;
        };
        if input.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host supplies a writable value or null.
        let Some(value) = (unsafe { value.as_mut() }) else { return kInvalidArgument };
        let mut units = Vec::new();
        for index in 0..128 {
            // SAFETY: VST3 supplies a terminated parameter string; stop at NUL, bounded to String128.
            let unit = unsafe { *input.wrapping_add(index) };
            if unit == 0 {
                break;
            }
            units.push(unit);
        }
        let Ok(text) = String::from_utf16(&units) else { return kResultFalse };
        let Ok(plain) = text.trim().parse::<f64>() else { return kResultFalse };
        if !plain.is_finite() {
            return kResultFalse;
        }
        *value = parameter.to_normalized(plain);
        kResultOk
    }
    unsafe fn normalizedParamToPlain(&self, id: u32, value: f64) -> f64 {
        self.parameters
            .index(id)
            .and_then(|index| self.parameters.info.params.get(index))
            .map(|parameter| parameter.from_normalized(value))
            .unwrap_or(0.0)
    }
    unsafe fn plainParamToNormalized(&self, id: u32, value: f64) -> f64 {
        self.parameters
            .index(id)
            .and_then(|index| self.parameters.info.params.get(index))
            .map(|parameter| parameter.to_normalized(value))
            .unwrap_or(0.0)
    }
    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        self.parameters
            .index(id)
            .and_then(|index| {
                self.parameters
                    .info
                    .params
                    .get(index)
                    .and_then(|parameter| self.parameters.value(index).map(|value| parameter.to_normalized(value)))
            })
            .unwrap_or(0.0)
    }
    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
        let Some(parameter) = self.parameters.index(id).and_then(|index| self.parameters.info.params.get(index)) else {
            return kInvalidArgument;
        };
        self.parameters.control_set(id, parameter.from_normalized(value));
        kResultOk
    }
    unsafe fn setComponentHandler(&self, _handler: *mut IComponentHandler) -> tresult {
        kResultOk
    }
    unsafe fn createView(&self, _name: *const c_char) -> *mut IPlugView {
        std::ptr::null_mut()
    }
}
