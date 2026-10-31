// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Panels for built-in effects, instruments and MIDI effects ([ADR 0021] › Amendment: device
//! panels for built-in devices). A built-in's parameters come from its native `DeviceInfo`; a
//! layout adds only the `.sd` format's UI part (a compact and an optional expanded canvas, and
//! how each parameter is presented), never a graph and never a parameter of its own. A device
//! without a layout gets a generated parameter list.
//!
//! [ADR 0021]: https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0021-device-graph-files.md

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use suhina_device_api::{DeviceInfo, ParamInfo, ParamKind, Scale, Unit};

use crate::ui::DeviceUi;
use crate::{DeviceSdkError, Parameter, ParameterControl, ParameterUi};

/// Width of the generated parameter list, and the default width of a compact panel.
pub const COMPACT_WIDTH: f32 = 260.0;
/// Height of every compact panel: the detail area's default 200 px less the device header and
/// its padding.
pub const COMPACT_HEIGHT: f32 = 168.0;
/// Tallest strip of controls in a device's header row.
pub const HEADER_HEIGHT: f32 = 20.0;
/// Widest strip of controls in a device's header row.
pub const HEADER_WIDTH: f32 = 480.0;
/// Largest layout text read.
pub const MAX_LAYOUT_BYTES: usize = crate::MAX_FILE_BYTES;

/// What the device panel draws for a built-in: its parameters and, when it has a layout, the
/// layout's canvases.
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    /// Every parameter of the device, in its order, as `ParamInfo` declares it, presented as
    /// the layout says (or as the generated list does).
    pub parameters: Vec<Parameter>,
    /// The compact canvas; `None` draws the generated list. Shared, so a panel kept while
    /// Suhina runs keeps one canvas whose images a view can cache.
    pub ui: Option<Arc<DeviceUi>>,
    /// The larger canvas opened from the device's header, if it has one.
    pub expanded: Option<Arc<DeviceUi>>,
    /// A strip of controls the host puts in the device's header row, right of its name.
    pub header: Option<Arc<DeviceUi>>,
}

/// A built-in device's layout: what a `devices/<name>/panel.json` holds, or what a device crate
/// builds in Rust. Parameters are named by their ids; a layout cannot declare one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Layout {
    /// The compact panel, 168 px high. `None` keeps the generated list in the chain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<DeviceUi>,
    /// The expanded panel, opened from the device's header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<DeviceUi>,
    /// Controls in the device's header row, right of its name: at most `HEADER_HEIGHT` high.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<DeviceUi>,
    /// How parameters are presented, by id: their control and the `.sd` parameter `ui` fields.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, Presentation>,
}

/// How one parameter is presented. Name, range, unit and default stay the device's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Presentation {
    /// The control that edits it, in place of the generated list's; refused when its range does
    /// not allow it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control: Option<ParameterControl>,
    /// Its look, as a `.sd` parameter's `ui` fields; a choice keeps the device's labels unless
    /// this gives its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<ParameterUi>,
}

impl Layout {
    /// Reads a layout from JSON (`devices/<name>/panel.json`).
    pub fn from_json(text: &str) -> Result<Self, DeviceSdkError> {
        if text.len() > MAX_LAYOUT_BYTES {
            return Err(DeviceSdkError(format!("a panel layout is limited to {} KiB", MAX_LAYOUT_BYTES / 1024)));
        }
        serde_json::from_str(text).map_err(|e| DeviceSdkError(format!("invalid panel layout: {e}")))
    }

    /// The panel this layout gives `info`'s device. Fails when the layout names a parameter id
    /// the device does not have, presents one with a control its range does not allow, or draws
    /// a canvas the `.sd` rules refuse.
    pub fn panel(&self, info: &DeviceInfo) -> Result<Panel, DeviceSdkError> {
        let fail = |e: DeviceSdkError| DeviceSdkError(format!("panel layout of {}: {e}", info.id));
        let mut parameters = parameters(info);
        for (id, presentation) in &self.parameters {
            let parameter = parameters
                .iter_mut()
                .find(|p| &p.id == id)
                .ok_or_else(|| fail(DeviceSdkError(format!("{} has no parameter {id:?}", info.name))))?;
            if let Some(control) = presentation.control {
                parameter.control = control;
            }
            if let Some(ui) = &presentation.ui {
                // A choice keeps the device's own labels unless the layout gives its own.
                let options = std::mem::take(&mut parameter.ui.options);
                parameter.ui = ui.clone();
                if parameter.ui.options.is_empty() {
                    parameter.ui.options = options;
                }
            }
        }
        crate::validate_parameters(&parameters).map_err(fail)?;
        for ui in [&self.ui, &self.expanded].into_iter().flatten() {
            ui.validate(&parameters).map_err(fail)?;
        }
        if let Some(header) = &self.header {
            if header.size[1] > HEADER_HEIGHT || header.size[0] > HEADER_WIDTH {
                return Err(fail(DeviceSdkError(format!(
                    "header controls larger than {HEADER_WIDTH} by {HEADER_HEIGHT} px"
                ))));
            }
            if header.controls.iter().any(|c| c.bounds[1] + c.bounds[3] > header.size[1]) {
                return Err(fail(DeviceSdkError("a header control runs below the header's strip".into())));
            }
            // A canvas is at least 24 px high; the header's strip is checked as one that tall.
            let mut checked = header.clone();
            checked.size[1] = 24.;
            checked.validate(&parameters).map_err(fail)?;
        }
        Ok(Panel {
            parameters,
            ui: self.ui.clone().map(Arc::new),
            expanded: self.expanded.clone().map(Arc::new),
            header: self.header.clone().map(Arc::new),
        })
    }
}

/// The generated panel: every parameter in the device's order, no canvas.
pub fn generated(info: &DeviceInfo) -> Panel {
    Panel { parameters: parameters(info), ui: None, expanded: None, header: None }
}

/// `info`'s parameters as the `.sd` format declares parameters, with the stock control for
/// each: a slider (logarithmic on a logarithmic scale), a toggle, a whole-number slider or a
/// menu of the choice's labels.
pub fn parameters(info: &DeviceInfo) -> Vec<Parameter> {
    info.params.iter().map(parameter).collect()
}

/// One native parameter as the generated list shows it: its stock control, its unit, and two
/// decimals (one in Hz and %, none on a stepped control).
fn parameter(p: &ParamInfo) -> Parameter {
    let mut parameter = Parameter::new(p.id, p.name, (p.min, p.max), p.default);
    // The native device smooths its own parameters.
    parameter.smoothing_ms = 0.;
    parameter.control = match p.kind {
        ParamKind::Continuous(Scale::Logarithmic) if p.min > 0.0 => ParameterControl::LogSlider,
        ParamKind::Continuous(_) => ParameterControl::Slider,
        ParamKind::Integer => ParameterControl::Integer,
        ParamKind::Toggle => ParameterControl::Toggle,
        ParamKind::Choice(options) => {
            parameter.ui.options = options.iter().map(|s| (*s).into()).collect();
            ParameterControl::Choice
        }
    };
    parameter.ui.decimals = match p.kind {
        ParamKind::Continuous(_) => match p.unit {
            Unit::Hertz | Unit::Percent => 1,
            _ => 2,
        },
        _ => 0,
    };
    parameter.unit = unit(p.unit).into();
    parameter
}

/// The unit's text as a `.sd` parameter writes it.
pub fn unit(unit: Unit) -> &'static str {
    match unit {
        Unit::None => "",
        Unit::Decibels => "dB",
        Unit::Hertz => "Hz",
        Unit::Seconds => "s",
        Unit::Percent => "%",
        Unit::Balance => "balance",
        Unit::Octaves => "oct",
        Unit::Semitones => "st",
        Unit::ScaleDegrees => "degrees",
        Unit::Key => "key",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{CustomControl, UiBinding, UiGesture};
    use suhina_device_api::Role;

    const PARAMS: [ParamInfo; 3] = [
        ParamInfo {
            id: "gain",
            name: "Gain",
            group: "",
            kind: ParamKind::Continuous(Scale::Linear),
            min: -24.0,
            max: 24.0,
            default: 0.0,
            unit: Unit::Decibels,
        },
        ParamInfo {
            id: "freq",
            name: "Frequency",
            group: "",
            kind: ParamKind::Continuous(Scale::Logarithmic),
            min: 20.0,
            max: 20000.0,
            default: 1000.0,
            unit: Unit::Hertz,
        },
        ParamInfo {
            id: "mode",
            name: "Mode",
            group: "",
            kind: ParamKind::Choice(&["Soft", "Hard"]),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: Unit::None,
        },
    ];
    const INFO: DeviceInfo = DeviceInfo { id: "test", name: "Test", role: Role::Effect, params: &PARAMS };

    fn knob(id: &str, parameter: &str) -> CustomControl {
        CustomControl {
            widget: None,
            visible_when: None,
            id: id.into(),
            name: id.into(),
            tooltip: String::new(),
            bounds: [0., 0., 40., 40.],
            gesture: UiGesture::Drag,
            x: None,
            y: Some(UiBinding::new(parameter)),
            drawing: vec![],
        }
    }

    #[test]
    fn the_generated_list_takes_every_parameter_from_the_device() {
        let panel = generated(&INFO);
        assert_eq!(panel.ui, None);
        let ids: Vec<_> = panel.parameters.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["gain", "freq", "mode"]);
        let [gain, freq, mode] = &panel.parameters[..] else { panic!("three parameters") };
        assert_eq!((gain.control, gain.unit.as_str(), gain.min, gain.max), (ParameterControl::Slider, "dB", -24., 24.));
        assert_eq!(freq.control, ParameterControl::LogSlider);
        assert_eq!(
            (mode.control, mode.ui.options.as_slice()),
            (ParameterControl::Choice, &["Soft", "Hard"].map(String::from)[..])
        );
        crate::validate_parameters(&panel.parameters).unwrap();
    }

    #[test]
    fn a_layout_presents_the_devices_parameters_and_round_trips() {
        let mut layout = Layout {
            ui: Some(DeviceUi { size: [COMPACT_WIDTH, COMPACT_HEIGHT], ..DeviceUi::default() }),
            ..Layout::default()
        };
        layout.ui.as_mut().unwrap().controls.push(knob("gain-knob", "gain"));
        layout.parameters.insert(
            "gain".into(),
            Presentation {
                control: Some(ParameterControl::Knob),
                ui: Some(ParameterUi { decimals: 1, ..ParameterUi::default() }),
            },
        );
        layout.parameters.insert("mode".into(), Presentation { control: Some(ParameterControl::Radio), ui: None });
        let text = serde_json::to_string(&layout).unwrap();
        assert_eq!(Layout::from_json(&text).unwrap(), layout);
        let panel = layout.panel(&INFO).unwrap();
        let gain = &panel.parameters[0];
        assert_eq!((gain.control, gain.ui.decimals, gain.min, gain.max), (ParameterControl::Knob, 1, -24., 24.));
        // A choice keeps its labels when the layout gives none.
        assert_eq!(panel.parameters[2].ui.options, ["Soft", "Hard"]);
        assert!(panel.ui.is_some());
    }

    #[test]
    fn a_layout_that_names_a_parameter_the_device_lacks_is_refused() {
        let mut layout = Layout::default();
        layout.parameters.insert("drive".into(), Presentation::default());
        let error = layout.panel(&INFO).unwrap_err();
        assert!(error.0.contains("no parameter \"drive\""), "{error}");

        let mut ui = DeviceUi::default();
        ui.controls.push(knob("drive-knob", "drive"));
        assert!(Layout { ui: Some(ui.clone()), ..Layout::default() }.panel(&INFO).is_err());
        assert!(Layout { expanded: Some(ui), ..Layout::default() }.panel(&INFO).is_err());
        let hidden = DeviceUi { hidden_parameters: vec!["drive".into()], ..DeviceUi::default() };
        assert!(Layout { ui: Some(hidden), ..Layout::default() }.panel(&INFO).is_err());
    }

    #[test]
    fn a_layout_cannot_give_a_parameter_a_control_its_range_refuses() {
        let mut layout = Layout::default();
        // -24..24 dB cannot be a logarithmic slider, nor a toggle.
        for control in [ParameterControl::LogSlider, ParameterControl::Toggle] {
            layout.parameters.insert("gain".into(), Presentation { control: Some(control), ui: None });
            assert!(layout.panel(&INFO).is_err(), "{control:?}");
        }
        // Nor can a layout declare a parameter or a graph: unknown fields are refused.
        assert!(Layout::from_json(r#"{"parameters":{"gain":{"min":0}}}"#).is_err());
        assert!(Layout::from_json(r#"{"nodes":[]}"#).is_err());
        assert!(Layout::from_json(&" ".repeat(MAX_LAYOUT_BYTES + 1)).is_err());
    }
}
