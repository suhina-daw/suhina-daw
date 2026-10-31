// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Build, validate, save, and run data-only Suhina device graphs (`.sd`).
//!
//! Version 4 separates device revisions from the file format version; version 5 adds an
//! optional expanded panel; version 6 a panel style sheet. A validated graph compiles off the
//! audio thread into a processor whose `process` method performs no allocation. File versions
//! 1 through 6 remain readable.

use std::fmt;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub mod author;
pub mod builtin;
pub mod editor;
mod graph;
mod midi;
mod modulation;
pub mod native_node;
mod nodes;
mod runtime;
pub mod sample_axis;
pub mod sample_resource;
pub mod sampler;
pub mod ui;
pub use midi::PreparedMidiEffect;
pub use nodes::granular::{grain_template, template as granular_template};
pub use nodes::templates::sampler_template;
pub use nodes::{
    FilterMode, NODE_CATALOG, NodeInfo, NodeKind, PortInfo, SampleInfoField, Waveform, node_reference_markdown,
};
pub use runtime::PreparedEffect;
pub use suhina_device_api::ProcessContext;

pub const EXTENSION: &str = "sd";
pub const FORMAT: &str = "suhina-device";
/// The version every file is written as, and the newest this crate reads. It rises with every
/// change to what a file holds, so an older Suhina refuses a newer file rather than dropping
/// what it does not know (ADR 0010 › Amendment: format versions, `docs/spec/sd-device-format.md`
/// › Versions). Files of every earlier version are still read. Version 6 adds a panel's style sheet.
pub const VERSION: u32 = 6;
pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_NODES: usize = 128;
pub const MAX_CONNECTIONS: usize = MAX_NODES * 5;
pub const MAX_DELAY_FRAMES: usize = 2_000_000;
pub const MAX_PARAMETERS: usize = 256;
pub const MAX_DEVICE_MIGRATIONS: usize = 128;
const MAX_TEXT_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceGraph {
    pub id: String,
    /// Revision of this device's identity and parameter contract, separate from the file format version.
    pub device_version: u32,
    pub name: String,
    pub role: DeviceRole,
    /// Direct migrations from older device revisions to this revision.
    pub migrations: Vec<DeviceMigration>,
    pub parameters: Vec<Parameter>,
    pub nodes: Vec<Node>,
    pub connections: Vec<Connection>,
    /// The compact panel, drawn in the device chain.
    pub ui: Option<ui::DeviceUi>,
    /// An optional larger panel, opened from a button in the device's header (version 5).
    pub expanded: Option<ui::DeviceUi>,
    pub editor: Option<editor::Editor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceMigration {
    pub from_version: u32,
    #[serde(default)]
    pub parameter_ids: Vec<ParameterIdMigration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterIdMigration {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    Effect,
    Instrument,
    MidiEffect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub id: String,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub control: ParameterControl,
    pub unit: String,
    pub ui: ParameterUi,
    pub smoothing_ms: f64,
}

/// How the parameter is presented in the generated device panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterControl {
    #[default]
    Slider,
    LogSlider,
    Integer,
    Toggle,
    Knob,
    LogKnob,
    Number,
    VerticalSlider,
    Button,
    Choice,
    Radio,
}

impl ParameterControl {
    /// Whether values advance in whole steps rather than continuously.
    pub fn is_stepped(self) -> bool {
        match self {
            Self::Integer | Self::Toggle | Self::Button | Self::Choice | Self::Radio => true,
            Self::Slider | Self::LogSlider | Self::Knob | Self::LogKnob | Self::Number | Self::VerticalSlider => false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelPosition {
    #[default]
    Left,
    Top,
    Hidden,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlShape {
    #[default]
    Rounded,
    Square,
    Pill,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterLink {
    pub enabled_by: String,
    pub peer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ParameterUi {
    /// Mirror edits to this peer while the named toggle is enabled.
    pub link: Option<ParameterLink>,
    pub tooltip: String,
    pub group: String,
    pub options: Vec<String>,
    pub step: f64,
    pub decimals: usize,
    pub size: Option<[f32; 2]>,
    pub position: Option<[f32; 2]>,
    pub accent: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
    pub panel_color: Option<[u8; 3]>,
    pub text_color: Option<[u8; 3]>,
    pub font_size: f32,
    pub monospace: bool,
    pub corner_radius: f32,
    pub shape: ControlShape,
    pub label_position: LabelPosition,
    pub show_value: bool,
}
impl Default for ParameterUi {
    fn default() -> Self {
        Self {
            link: None,
            tooltip: String::new(),
            group: String::new(),
            options: Vec::new(),
            step: 0.0,
            decimals: 3,
            size: None,
            position: None,
            accent: None,
            background: None,
            panel_color: None,
            text_color: None,
            font_size: 12.0,
            monospace: false,
            corner_radius: 4.0,
            shape: ControlShape::Rounded,
            label_position: LabelPosition::Left,
            show_value: true,
        }
    }
}
fn default_smoothing_ms() -> f64 {
    20.0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterBinding {
    pub parameter: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: String,
    pub bindings: Vec<ParameterBinding>,
    pub kind: NodeKind,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub from: String,
    pub to: String,
    #[serde(default = "default_output_port")]
    pub from_port: String,
    #[serde(default = "default_input_port")]
    pub to_port: String,
}
fn default_output_port() -> String {
    "out".into()
}
fn default_input_port() -> String {
    "in".into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSdkError(pub String);

impl fmt::Display for DeviceSdkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DeviceSdkError {}

impl DeviceGraph {
    pub fn new(id: impl Into<String>, name: impl Into<String>, role: DeviceRole) -> Self {
        Self {
            id: id.into(),
            device_version: 1,
            name: name.into(),
            role,
            migrations: Vec::new(),
            parameters: Vec::new(),
            nodes: Vec::new(),
            connections: Vec::new(),
            ui: None,
            expanded: None,
            editor: None,
        }
    }

    pub fn validate(&self) -> Result<(), DeviceSdkError> {
        if self.id.is_empty()
            || self.id.len() > MAX_TEXT_BYTES
            || self.name.is_empty()
            || self.name.len() > MAX_TEXT_BYTES
        {
            return Err(DeviceSdkError("device id and name must be 1 to 128 bytes".into()));
        }
        if self.device_version == 0 || self.migrations.len() > MAX_DEVICE_MIGRATIONS {
            return Err(DeviceSdkError(format!(
                "device version must be positive and have at most {MAX_DEVICE_MIGRATIONS} migrations"
            )));
        }
        if self.nodes.len() < 2 || self.nodes.len() > MAX_NODES {
            return Err(DeviceSdkError(format!("a graph must have 2 to {MAX_NODES} nodes")));
        }
        if self.connections.len() > MAX_CONNECTIONS {
            return Err(DeviceSdkError(format!("a graph may have at most {MAX_CONNECTIONS} connections")));
        }
        if self.parameters.len() > MAX_PARAMETERS {
            return Err(DeviceSdkError(format!("a graph may have at most {MAX_PARAMETERS} parameters")));
        }

        validate_parameters(&self.parameters)?;
        let param_ids: std::collections::HashSet<&str> = self.parameters.iter().map(|p| p.id.as_str()).collect();

        let mut migration_versions = std::collections::HashSet::new();
        for migration in &self.migrations {
            if migration.from_version == 0
                || migration.from_version >= self.device_version
                || !migration_versions.insert(migration.from_version)
                || migration.parameter_ids.len() > MAX_PARAMETERS
            {
                return Err(DeviceSdkError("device migrations must uniquely target older positive versions".into()));
            }
            let mut from_ids = std::collections::HashSet::new();
            let mut to_ids = std::collections::HashSet::new();
            for mapping in &migration.parameter_ids {
                if mapping.from.is_empty()
                    || mapping.from.len() > MAX_TEXT_BYTES
                    || mapping.to.is_empty()
                    || mapping.to.len() > MAX_TEXT_BYTES
                    || mapping.from == mapping.to
                    || !from_ids.insert(mapping.from.as_str())
                    || !to_ids.insert(mapping.to.as_str())
                    || param_ids.contains(mapping.from.as_str())
                    || !param_ids.contains(mapping.to.as_str())
                {
                    return Err(DeviceSdkError(
                        "parameter migrations must uniquely rename old ids to current parameter ids".into(),
                    ));
                }
            }
        }
        if self.device_version > 1 && !migration_versions.contains(&(self.device_version - 1)) {
            return Err(DeviceSdkError(
                "each device version bump must include a migration from the previous version".into(),
            ));
        }

        for ui in [&self.ui, &self.expanded].into_iter().flatten() {
            ui.validate(&self.parameters)?;
        }
        self.validate_nodes()?;
        self.execution_order()?;
        if let Some(editor) = &self.editor {
            editor.validate(self)?;
        }
        self.delay_budget(384_000.0)?;
        Ok(())
    }

    /// Dependency order; delay input edges are committed after the current frame.
    pub fn ordered_nodes(&self) -> Vec<&Node> {
        self.execution_order().unwrap_or_default().into_iter().filter_map(|i| self.nodes.get(i)).collect()
    }

    pub fn to_sd(&self) -> Result<String, DeviceSdkError> {
        self.validate()?;
        let file = FileV1::from(self);
        let text = serde_json::to_string_pretty(&file)
            .map_err(|error| DeviceSdkError(format!("could not encode device graph: {error}")))?;
        if text.len() > MAX_FILE_BYTES {
            return Err(DeviceSdkError("encoded .sd exceeds 2 MiB".into()));
        }
        Ok(text)
    }

    /// Carry saved values into this revision. Unchanged ids are retained; renamed ids need an explicit migration.
    pub fn migrate_parameter_values(
        &self,
        from_version: u32,
        values: &[(String, f64)],
    ) -> Result<Vec<(String, f64)>, DeviceSdkError> {
        self.validate()?;
        if from_version == 0 || from_version > self.device_version {
            return Err(DeviceSdkError("source device version must be positive and no newer than the target".into()));
        }
        if values.len() > MAX_PARAMETERS {
            return Err(DeviceSdkError(format!("saved parameter values may not exceed {MAX_PARAMETERS}")));
        }
        let mut saved = std::collections::HashMap::with_capacity(values.len());
        for (id, value) in values {
            if id.is_empty() || id.len() > MAX_TEXT_BYTES || saved.insert(id.as_str(), *value).is_some() {
                return Err(DeviceSdkError("saved parameter ids must be unique and 1..128 bytes".into()));
            }
        }
        let migration = if from_version == self.device_version {
            None
        } else {
            Some(self.migrations.iter().find(|migration| migration.from_version == from_version).ok_or_else(|| {
                DeviceSdkError(format!(
                    "device {} version {} has no migration from version {from_version}",
                    self.id, self.device_version
                ))
            })?)
        };
        Ok(self
            .parameters
            .iter()
            .map(|parameter| {
                let old_id = migration
                    .and_then(|migration| migration.parameter_ids.iter().find(|mapping| mapping.to == parameter.id))
                    .map_or(parameter.id.as_str(), |mapping| mapping.from.as_str());
                let value = saved.get(old_id).map_or(parameter.default, |value| parameter.clamp(*value));
                (parameter.id.clone(), value)
            })
            .collect())
    }

    /// Expand one command into an atomic set of clamped values, off the audio thread.
    pub fn parameter_edits(&self, id: &str, value: f64, current: impl Fn(&str) -> f64) -> Vec<(usize, f64)> {
        let Some((index, p)) = self.parameters.iter().enumerate().find(|(_, p)| p.id == id) else { return Vec::new() };
        let value = p.clamp(value);
        let mut changes = vec![(index, value)];
        for (i, source) in self.parameters.iter().enumerate() {
            let Some(link) = &source.ui.link else { continue };
            let Some((j, peer)) = self.parameters.iter().enumerate().find(|(_, p)| p.id == link.peer) else { continue };
            if source.id == id && current(&link.enabled_by) > 0.5 {
                changes.push((j, peer.clamp(value)));
            } else if link.enabled_by == id && value > 0.5 && i < j {
                changes.push((j, peer.clamp(current(&source.id))));
            }
        }
        changes
    }

    pub fn from_sd(text: &str) -> Result<Self, DeviceSdkError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(DeviceSdkError(format!("a .sd file is limited to {} KiB", MAX_FILE_BYTES / 1024)));
        }
        let file: FileV1 =
            serde_json::from_str(text).map_err(|error| DeviceSdkError(format!("invalid .sd JSON: {error}")))?;
        if file.format != FORMAT || !(1..=VERSION).contains(&file.version) {
            return Err(DeviceSdkError(format!(
                "unsupported device format/version: {:?}/{}",
                file.format, file.version
            )));
        }
        if file.version < 3 && file.editor.is_some() {
            return Err(DeviceSdkError("editor metadata requires .sd version 3".into()));
        }
        if file.version < 5 && file.device.expanded.is_some() {
            return Err(DeviceSdkError("an expanded panel requires .sd version 5".into()));
        }
        if file.version < 6 && file.device.ui.iter().chain(file.device.expanded.iter()).any(|ui| ui.style.is_some()) {
            return Err(DeviceSdkError("a panel style sheet requires .sd version 6".into()));
        }
        if file.version == 1
            && (file
                .nodes
                .iter()
                .any(|node| !matches!(node.kind, NodeKind::AudioInput | NodeKind::AudioOutput | NodeKind::Gain { .. }))
                || file.connections.iter().any(|edge| edge.from_port != "out" || edge.to_port != "in"))
        {
            return Err(DeviceSdkError("version 1 only supports legacy input/gain/output nodes".into()));
        }
        if file.version == 1 {
            let mut sources = std::collections::HashSet::new();
            let mut destinations = std::collections::HashSet::new();
            if file.connections.iter().any(|edge| !sources.insert(&edge.from) || !destinations.insert(&edge.to)) {
                return Err(DeviceSdkError("version 1 connections must form a linear chain".into()));
            }
        }
        let graph: Self = file.into();
        graph.validate()?;
        Ok(graph)
    }

    pub fn read_file(path: &Path) -> Result<Self, DeviceSdkError> {
        let file = std::fs::File::open(path)
            .map_err(|error| DeviceSdkError(format!("could not open {}: {error}", path.display())))?;
        let mut text = String::new();
        file.take((MAX_FILE_BYTES + 1) as u64)
            .read_to_string(&mut text)
            .map_err(|error| DeviceSdkError(format!("could not read {}: {error}", path.display())))?;
        if text.len() > MAX_FILE_BYTES {
            return Err(DeviceSdkError(format!("a .sd file is limited to {} KiB", MAX_FILE_BYTES / 1024)));
        }
        Self::from_sd(&text)
    }

    pub fn write_file(&self, path: &Path) -> Result<(), DeviceSdkError> {
        let text = self.to_sd()?;
        let failed = |error: std::io::Error| DeviceSdkError(format!("could not save {}: {error}", path.display()));
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(failed)?;
        }
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".saving");
        let temporary = std::path::PathBuf::from(temporary);
        std::fs::write(&temporary, text).map_err(failed)?;
        std::fs::rename(&temporary, path).map_err(|error| {
            let _gone = std::fs::remove_file(&temporary);
            failed(error)
        })
    }

    /// Prepare at 48 kHz; use compile_at for the actual engine sample rate.
    pub fn compile(&self) -> Result<PreparedEffect, DeviceSdkError> {
        self.compile_at(48_000.0)
    }
    /// Compiles off the audio thread, including bounded delay storage.
    pub fn compile_at(&self, sample_rate: f64) -> Result<PreparedEffect, DeviceSdkError> {
        self.validate()?;
        if self.role != DeviceRole::Effect {
            return Err(DeviceSdkError("compile_at prepares audio-effect graphs only".into()));
        }
        PreparedEffect::new(self, sample_rate)
    }

    /// Compiles a MIDI-event graph off the audio thread.
    pub fn compile_midi(&self) -> Result<PreparedMidiEffect, DeviceSdkError> {
        if self.role != DeviceRole::MidiEffect {
            return Err(DeviceSdkError("compile_midi prepares midi_effect graphs only".into()));
        }
        self.validate()?;
        PreparedMidiEffect::new(self)
    }

    /// Compiles an instrument graph. MIDI events are supplied with their frame offsets at render time.
    pub fn compile_instrument_at(&self, sample_rate: f64) -> Result<PreparedEffect, DeviceSdkError> {
        if self.role != DeviceRole::Instrument {
            return Err(DeviceSdkError("compile_instrument_at prepares instrument graphs only".into()));
        }
        self.validate()?;
        PreparedEffect::new(self, sample_rate)
    }
}

impl Parameter {
    pub fn options(mut self, options: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.ui.options = options.into_iter().map(Into::into).collect();
        self
    }

    pub fn linked(mut self, peer: impl Into<String>, enabled_by: impl Into<String>) -> Self {
        self.ui.link = Some(ParameterLink { peer: peer.into(), enabled_by: enabled_by.into() });
        self
    }

    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    pub fn control(mut self, control: ParameterControl) -> Self {
        self.control = control;
        self
    }

    pub fn smoothing_ms(mut self, milliseconds: f64) -> Self {
        self.smoothing_ms = milliseconds;
        self
    }

    /// Whether its control turns logarithmically (a log slider or knob over a positive range).
    pub fn logarithmic(&self) -> bool {
        matches!(self.control, ParameterControl::LogSlider | ParameterControl::LogKnob) && self.min > 0.0
    }

    /// Where `value` sits on an automation lane, 0 … 1: linear, or logarithmic for a log
    /// control; the middle for a parameter with no range.
    pub fn lane_position(&self, value: f64) -> f64 {
        let (min, max) = (self.min, self.max);
        let value = self.clamp(value);
        if min >= max {
            0.5
        } else if self.logarithmic() {
            (value.ln() - min.ln()) / (max.ln() - min.ln())
        } else {
            (value - min) / (max - min)
        }
    }

    /// The value at `position` (0 … 1) on an automation lane: the inverse of
    /// [`Parameter::lane_position`], held in the range.
    pub fn lane_value(&self, position: f64) -> f64 {
        let (min, max) = (self.min, self.max);
        let position = if position.is_finite() { position.clamp(0.0, 1.0) } else { 0.0 };
        let value = if min >= max {
            min
        } else if self.logarithmic() {
            (min.ln() + position * (max.ln() - min.ln())).exp()
        } else {
            min + position * (max - min)
        };
        self.clamp(value)
    }

    /// Clamp hostile values and preserve discrete parameter semantics.
    pub fn clamp(&self, value: f64) -> f64 {
        let value = if value.is_finite() { value.clamp(self.min, self.max) } else { self.default };
        if self.control.is_stepped() { value.round().clamp(self.min, self.max) } else { value }
    }

    pub fn new(id: impl Into<String>, name: impl Into<String>, range: (f64, f64), default: f64) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            min: range.0,
            max: range.1,
            default,
            control: ParameterControl::Slider,
            unit: String::new(),
            ui: ParameterUi::default(),
            smoothing_ms: default_smoothing_ms(),
        }
    }
}

impl Node {
    pub fn bind(mut self, parameter: impl Into<String>, target: impl Into<String>) -> Self {
        self.bindings.push(ParameterBinding { parameter: parameter.into(), target: target.into() });
        self
    }

    pub fn new(id: impl Into<String>, kind: NodeKind) -> Self {
        Self { id: id.into(), kind, visible: true, bindings: Vec::new() }
    }
    pub fn audio_input(id: impl Into<String>) -> Self {
        Self { id: id.into(), kind: NodeKind::AudioInput, visible: true, bindings: Vec::new() }
    }
    pub fn gain(id: impl Into<String>, parameter: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: NodeKind::Gain { parameter: parameter.into() },
            visible: true,
            bindings: Vec::new(),
        }
    }
    pub fn audio_output(id: impl Into<String>) -> Self {
        Self { id: id.into(), kind: NodeKind::AudioOutput, visible: true, bindings: Vec::new() }
    }
}

impl Connection {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self::ports(from, "out", to, "in")
    }
    pub fn ports(
        from: impl Into<String>,
        from_port: impl Into<String>,
        to: impl Into<String>,
        to_port: impl Into<String>,
    ) -> Self {
        Self { from: from.into(), to: to.into(), from_port: from_port.into(), to_port: to_port.into() }
    }
}

#[derive(Serialize, Deserialize)]
struct FileV1 {
    format: String,
    version: u32,
    device: DeviceV1,
    nodes: Vec<NodeV1>,
    connections: Vec<Connection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    editor: Option<editor::Editor>,
}

#[derive(Serialize, Deserialize)]
struct DeviceV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ui: Option<ui::DeviceUi>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expanded: Option<ui::DeviceUi>,
    id: String,
    #[serde(default = "initial_device_version")]
    version: u32,
    #[serde(default)]
    migrations: Vec<DeviceMigration>,
    name: String,
    role: DeviceRole,
    parameters: Vec<ParameterV1>,
}

fn initial_device_version() -> u32 {
    1
}

#[derive(Serialize, Deserialize)]
struct ParameterV1 {
    id: String,
    name: String,
    min: f64,
    max: f64,
    default: f64,
    #[serde(default)]
    control: ParameterControl,
    #[serde(default)]
    unit: String,
    #[serde(default)]
    ui: ParameterUi,
    #[serde(default = "default_smoothing_ms")]
    smoothing_ms: f64,
}

#[derive(Serialize, Deserialize)]
struct NodeV1 {
    id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bindings: Vec<ParameterBinding>,
    #[serde(default = "default_visible", skip_serializing_if = "is_visible")]
    visible: bool,
    #[serde(flatten)]
    kind: NodeKind,
}

fn default_visible() -> bool {
    true
}

fn is_visible(visible: &bool) -> bool {
    *visible
}

impl From<&DeviceGraph> for FileV1 {
    fn from(graph: &DeviceGraph) -> Self {
        let nodes = graph
            .nodes
            .iter()
            .map(|node| NodeV1 {
                id: node.id.clone(),
                visible: node.visible,
                kind: node.kind.clone(),
                bindings: node.bindings.clone(),
            })
            .collect();
        Self {
            format: FORMAT.into(),
            version: VERSION,
            device: DeviceV1 {
                ui: graph.ui.clone(),
                expanded: graph.expanded.clone(),
                id: graph.id.clone(),
                version: graph.device_version,
                migrations: graph.migrations.clone(),
                name: graph.name.clone(),
                role: graph.role,
                parameters: graph
                    .parameters
                    .iter()
                    .map(|parameter| ParameterV1 {
                        id: parameter.id.clone(),
                        name: parameter.name.clone(),
                        min: parameter.min,
                        max: parameter.max,
                        default: parameter.default,
                        control: parameter.control,
                        unit: parameter.unit.clone(),
                        ui: parameter.ui.clone(),
                        smoothing_ms: parameter.smoothing_ms,
                    })
                    .collect(),
            },
            nodes,
            connections: graph.connections.clone(),
            editor: graph.editor.clone(),
        }
    }
}

impl From<FileV1> for DeviceGraph {
    fn from(file: FileV1) -> Self {
        Self {
            ui: file.device.ui,
            expanded: file.device.expanded,
            editor: file.editor,
            id: file.device.id,
            device_version: file.device.version,
            name: file.device.name,
            role: file.device.role,
            migrations: file.device.migrations,
            parameters: file
                .device
                .parameters
                .into_iter()
                .map(|parameter| Parameter {
                    id: parameter.id,
                    name: parameter.name,
                    min: parameter.min,
                    max: parameter.max,
                    default: parameter.default,
                    control: parameter.control,
                    unit: parameter.unit,
                    ui: parameter.ui,
                    smoothing_ms: parameter.smoothing_ms,
                })
                .collect(),
            nodes: file
                .nodes
                .into_iter()
                .map(|node| Node { id: node.id, visible: node.visible, kind: node.kind, bindings: node.bindings })
                .collect(),
            connections: file.connections,
        }
    }
}

/// Checks a parameter list as a `.sd` file declares it: ids, ranges, controls and their look.
/// A built-in device's panel runs its parameters through it too ([`builtin`]).
pub(crate) fn validate_parameters(parameters: &[Parameter]) -> Result<(), DeviceSdkError> {
    let mut param_ids = std::collections::HashSet::new();
    for parameter in parameters {
        if parameter.id.is_empty()
            || parameter.id.len() > MAX_TEXT_BYTES
            || parameter.name.is_empty()
            || parameter.name.len() > MAX_TEXT_BYTES
            || !param_ids.insert(parameter.id.as_str())
        {
            return Err(DeviceSdkError(
                "parameter ids and names must be unique, non-empty, and at most 128 bytes".into(),
            ));
        }
        if !parameter.min.is_finite()
            || !parameter.max.is_finite()
            || !parameter.default.is_finite()
            || parameter.min < -(f32::MAX as f64)
            || parameter.max > f32::MAX as f64
            || parameter.min > parameter.max
            || !(parameter.min..=parameter.max).contains(&parameter.default)
        {
            return Err(DeviceSdkError(format!("parameter {:?} has an invalid range or default", parameter.id)));
        }
        if parameter.unit.len() > MAX_TEXT_BYTES {
            return Err(DeviceSdkError(format!("parameter {:?}: unit exceeds 128 bytes", parameter.id)));
        }
        let ui = &parameter.ui;
        if let Some(link) = &ui.link {
            let peer = parameters.iter().find(|p| p.id == link.peer);
            let gate = parameters.iter().find(|p| p.id == link.enabled_by);
            if !peer.is_some_and(|p| {
                p.id != parameter.id
                    && p.min == parameter.min
                    && p.max == parameter.max
                    && p.control == parameter.control
                    && p.unit == parameter.unit
            }) || !gate.is_some_and(|p| p.control == ParameterControl::Toggle && p.min == 0.0 && p.max == 1.0)
            {
                return Err(DeviceSdkError("parameter link needs a matching peer and a 0..1 toggle".into()));
            }
        }
        let minimum_height =
            ui.font_size + 16.0 + if ui.label_position == LabelPosition::Top { ui.font_size + 4.0 } else { 0.0 };
        if ui.size.is_some_and(|[w, h]| {
            !w.is_finite() || !h.is_finite() || !(24.0..=4096.0).contains(&w) || !(minimum_height..=4096.0).contains(&h)
        }) || ui.position.is_some_and(|p| p.iter().any(|v| !v.is_finite() || !(0.0..=8192.0).contains(v)))
            || !ui.font_size.is_finite()
            || !(8.0..=48.0).contains(&ui.font_size)
            || !ui.corner_radius.is_finite()
            || !(0.0..=128.0).contains(&ui.corner_radius)
        {
            return Err(DeviceSdkError(format!("parameter {:?}: invalid UI geometry or font", parameter.id)));
        }
        if !parameter.smoothing_ms.is_finite()
            || !(0.0..=1000.0).contains(&parameter.smoothing_ms)
            || !parameter.ui.step.is_finite()
            || parameter.ui.step < 0.0
            || parameter.ui.decimals > 8
            || parameter.ui.tooltip.len() > 1024
            || parameter.ui.group.len() > 128
            || parameter.ui.options.len() > 128
            || parameter.ui.options.iter().any(|s| s.is_empty() || s.len() > 128)
        {
            return Err(DeviceSdkError(format!("parameter {:?}: invalid UI or smoothing settings", parameter.id)));
        }
        if matches!(parameter.control, ParameterControl::Choice | ParameterControl::Radio)
            && (parameter.ui.options.is_empty()
                || parameter.min != 0.0
                || parameter.max != (parameter.ui.options.len() - 1) as f64)
        {
            return Err(DeviceSdkError(format!(
                "parameter {:?}: choices require range 0..options.len()-1",
                parameter.id
            )));
        }
        let discrete = parameter.control.is_stepped();
        if (discrete && [parameter.min, parameter.max, parameter.default].iter().any(|v| v.fract() != 0.0))
            || (matches!(parameter.control, ParameterControl::Toggle | ParameterControl::Button)
                && (parameter.min != 0.0 || parameter.max != 1.0))
            || (matches!(parameter.control, ParameterControl::LogSlider | ParameterControl::LogKnob)
                && parameter.min <= 0.0)
        {
            return Err(DeviceSdkError(format!(
                "parameter {:?}: range/default incompatible with control",
                parameter.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
