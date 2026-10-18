// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Convenience constructors produce the existing serialized controls, never a second UI model.
use super::*;
use crate::ParameterControl;

/// An authoring-only origin shared by controls with local bounds. Rebuild the panel after moving it.
#[derive(Debug, Clone, Copy)]
pub struct ControlGroup {
    origin: [f32; 2],
}

impl ControlGroup {
    pub fn new(origin: [f32; 2]) -> Result<Self, DeviceSdkError> {
        if origin.iter().any(|v| !v.is_finite() || !(0.0..=8192.0).contains(v)) {
            return Err(DeviceSdkError("control group origin must be finite and within 0..8192".into()));
        }
        Ok(Self { origin })
    }

    /// Flatten local control bounds into canvas bounds, preserving drawing coordinates and order.
    pub fn place(&self, mut control: CustomControl) -> Result<CustomControl, DeviceSdkError> {
        let [x, y, width, height] = control.bounds;
        let left = self.origin[0] + x;
        let top = self.origin[1] + y;
        if control.bounds.iter().any(|v| !v.is_finite())
            || x < 0.
            || y < 0.
            || width < 8.
            || height < 8.
            || left + width > 8192.
            || top + height > 8192.
        {
            return Err(DeviceSdkError(format!("control {:?}: invalid group-local bounds", control.id)));
        }
        control.bounds = [left, top, width, height];
        Ok(control)
    }
}

impl UiValue {
    pub fn equals(self, value: impl Into<UiValue>) -> Self {
        Self::Expression(Box::new(UiExpression::Equal { a: self, b: value.into() }))
    }
}

impl UiBinding {
    pub fn logarithmic(mut self, logarithmic: bool) -> Self {
        self.logarithmic = logarithmic;
        self
    }
}

impl CustomControl {
    pub fn bounds(mut self, bounds: [f32; 4]) -> Self {
        self.bounds = bounds;
        self
    }

    /// Numeric setters reject incompatible widgets instead of silently ignoring authoring mistakes.
    pub fn decimals(mut self, precision: usize) -> Result<Self, DeviceSdkError> {
        if precision > 8 {
            return Err(DeviceSdkError("control precision must be within 0..8".into()));
        }
        match &mut self.widget {
            Some(Widget::Number { decimals, .. } | Widget::Dial { decimals, .. } | Widget::Stepper { decimals }) => {
                *decimals = precision
            }
            _ => return Err(DeviceSdkError(format!("control {:?}: widget does not support precision", self.id))),
        }
        Ok(self)
    }

    pub fn format(mut self, value: TextFormat) -> Result<Self, DeviceSdkError> {
        match &mut self.widget {
            Some(Widget::Number { format, .. } | Widget::Dial { format, .. }) => *format = value,
            _ => return Err(DeviceSdkError(format!("control {:?}: widget does not support a text format", self.id))),
        }
        Ok(self)
    }

    pub fn prefix(mut self, value: impl Into<String>) -> Result<Self, DeviceSdkError> {
        match &mut self.widget {
            Some(Widget::Number { prefix, .. }) => *prefix = value.into(),
            _ => return Err(DeviceSdkError(format!("control {:?}: widget does not support a prefix", self.id))),
        }
        Ok(self)
    }

    pub fn off_label(mut self, value: impl Into<String>) -> Result<Self, DeviceSdkError> {
        match &mut self.widget {
            Some(Widget::Button { off_label, .. }) => *off_label = Some(value.into()),
            _ => return Err(DeviceSdkError(format!("control {:?}: widget does not support an off label", self.id))),
        }
        Ok(self)
    }
    /// A passive widget. Interactive constructors below add the matching gesture and binding.
    pub fn widget(id: impl Into<String>, name: impl Into<String>, widget: Widget, bounds: [f32; 4]) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            widget: Some(widget),
            bounds,
            tooltip: String::new(),
            visible_when: None,
            gesture: UiGesture::Drag,
            x: None,
            y: None,
            drawing: Vec::new(),
        }
    }

    pub fn number(parameter: impl Into<String>, bounds: [f32; 4]) -> Self {
        let parameter = parameter.into();
        let mut control = Self::widget(
            &parameter,
            parameter.replace('_', " "),
            Widget::Number {
                decimals: 0,
                format: TextFormat::Number,
                color: WidgetPalette::default().text,
                center: false,
                bordered: true,
                prefix: String::new(),
                suffix: String::new(),
            },
            bounds,
        );
        control.y = Some(UiBinding::new(parameter));
        control.tooltip = "Drag vertically; Shift for precision; double click resets".into();
        control
    }

    pub fn dial(parameter: impl Into<String>, label: impl Into<String>, bounds: [f32; 4]) -> Self {
        let parameter = parameter.into();
        let label = label.into();
        let mut control = Self::number(&parameter, bounds);
        control.name = if label.is_empty() { parameter.replace('_', " ") } else { label.replace('\n', " ") };
        control.widget = Some(Widget::Dial { label, decimals: 0, format: TextFormat::Number });
        control
    }

    pub fn toggle(parameter: impl Into<String>, label: impl Into<String>, bounds: [f32; 4]) -> Self {
        let parameter = parameter.into();
        let label = label.into();
        let mut control = Self::widget(
            &parameter,
            &label,
            Widget::Button { label: label.clone(), off_label: None, indicator: false },
            bounds,
        );
        control.tooltip = format!("{parameter}: {label}");
        control.gesture = UiGesture::Toggle;
        control.x = Some(UiBinding::new(parameter));
        control
    }

    pub fn set_button(parameter: impl Into<String>, value: u16, label: impl Into<String>, bounds: [f32; 4]) -> Self {
        let mut control = Self::toggle(parameter, label, bounds);
        control.id = format!("{}_{value}", control.id);
        control.gesture = UiGesture::Set(value);
        control
    }

    pub fn menu(parameter: impl Into<String>, bounds: [f32; 4]) -> Self {
        let parameter = parameter.into();
        let mut control = Self::widget(&parameter, parameter.replace('_', " "), Widget::Menu, bounds);
        control.gesture = UiGesture::Menu;
        control.x = Some(UiBinding::new(parameter));
        control
    }

    pub fn value_menu(parameter: impl Into<String>, choices: Vec<(u16, String)>, bounds: [f32; 4]) -> Self {
        let mut control = Self::menu(parameter, bounds);
        control.widget = Some(Widget::ValueMenu { choices });
        control
    }

    pub fn label(id: impl Into<String>, label: impl Into<String>, bounds: [f32; 4]) -> Self {
        let label = label.into();
        Self::widget(id, &label, Widget::Label { label: label.clone() }, bounds)
    }

    pub fn separator(id: impl Into<String>, bounds: [f32; 4]) -> Self {
        Self::widget(id, "Section divider", Widget::Separator, bounds)
    }

    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = tooltip.into();
        self
    }
    pub fn visible_when(mut self, condition: UiValue) -> Self {
        self.visible_when = Some(condition);
        self
    }

    pub fn logarithmic(mut self, logarithmic: bool) -> Self {
        for binding in [&mut self.x, &mut self.y].into_iter().flatten() {
            binding.logarithmic = logarithmic;
        }
        self
    }

    /// Reuse a bound parameter's name and declared mapping, without changing control identity.
    /// Menus obtain choices and value units from this same parameter in the host.
    pub fn parameter_metadata(mut self, parameter: &Parameter) -> Self {
        let mut matched = false;
        for binding in [&mut self.x, &mut self.y].into_iter().flatten() {
            if binding.parameter == parameter.id {
                binding.logarithmic =
                    matches!(parameter.control, ParameterControl::LogKnob | ParameterControl::LogSlider);
                matched = true;
            }
        }
        if matched {
            self.name = parameter.name.clone();
            match &mut self.widget {
                Some(Widget::Number { decimals, format, .. } | Widget::Dial { decimals, format, .. }) => {
                    *decimals = parameter.ui.decimals;
                    *format = if matches!(parameter.control, ParameterControl::Choice | ParameterControl::Radio) {
                        TextFormat::Choice
                    } else if parameter.unit == "Hz" {
                        TextFormat::Frequency
                    } else {
                        TextFormat::Number
                    };
                }
                Some(Widget::Stepper { decimals }) => *decimals = parameter.ui.decimals,
                _ => {}
            }
        }
        self
    }
}
