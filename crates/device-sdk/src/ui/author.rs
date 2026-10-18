// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Convenience constructors produce the existing serialized controls, never a second UI model.
use super::*;
use crate::ParameterControl;

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
        }
        self
    }
}
