// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded off-thread node and component preparation; never used by audio processing.
use crate::native_node::{NativeNode, prepare_with};
use crate::{DeviceSdkError, NodeKind, PreparedEffect};
/// Most grain graphs one prepared device holds in all, across its voices.
pub(crate) const MAX_GRAINS_IN_ALL: usize = 256;
#[derive(Clone, Copy, Default)]
struct Scope {
    depth: usize,
    voice: bool,
    grain: bool,
}
#[derive(Default)]
pub(crate) struct Budget {
    nodes: usize,
    voices: usize,
    delays: usize,
    grains: usize,
    bindings: usize,
}
pub(crate) fn compile_graph(
    graph: &crate::DeviceGraph,
    rate: f64,
    samples: Option<&crate::sample_resource::PreparedSamples>,
    resolve: &mut impl FnMut(&str, f64) -> Result<Box<dyn NativeNode>, DeviceSdkError>,
) -> Result<PreparedEffect, DeviceSdkError> {
    compile_nested(graph, rate, samples, resolve, Scope::default(), &mut Budget::default())
}
fn compile_nested(
    graph: &crate::DeviceGraph,
    rate: f64,
    samples: Option<&crate::sample_resource::PreparedSamples>,
    resolve: &mut impl FnMut(&str, f64) -> Result<Box<dyn NativeNode>, DeviceSdkError>,
    scope: Scope,
    budget: &mut Budget,
) -> Result<PreparedEffect, DeviceSdkError> {
    if scope.depth > 4 {
        return Err(DeviceSdkError("component nesting exceeds four".into()));
    }
    // Public compilation validates the root; embedded children are validated by from_sd.
    if graph.role == crate::DeviceRole::MidiEffect {
        return Err(DeviceSdkError("audio components cannot prepare MIDI-effect graphs".into()));
    }
    budget.nodes = budget.nodes.saturating_add(graph.nodes.len());
    budget.bindings = budget.bindings.saturating_add(graph.nodes.iter().map(|n| n.bindings.len()).sum::<usize>());
    if budget.bindings > 16384 {
        return Err(DeviceSdkError("expanded parameter binding budget exceeded".into()));
    }
    budget.delays = budget.delays.saturating_add(graph.delay_budget(rate)?);
    budget.voices = budget.voices.saturating_add(
        graph.nodes.iter().filter(|n| matches!(n.kind, NodeKind::Sampler | NodeKind::SamplerAsset { .. })).count() * 32,
    );
    budget.voices = budget
        .voices
        .saturating_add(graph.nodes.iter().filter(|n| matches!(n.kind, NodeKind::TimeStretchPlayer { .. })).count());
    if budget.nodes > 4096 || budget.delays > crate::MAX_DELAY_FRAMES || budget.voices > 128 {
        return Err(DeviceSdkError("expanded graph state budget exceeded".into()));
    }
    let resources = if graph.nodes.iter().any(|n| n.kind.requires_samples()) {
        Some(crate::nodes::resources::Resources::new(
            graph,
            samples.ok_or_else(|| DeviceSdkError("sample bindings are required".into()))?,
        )?)
    } else {
        None
    };
    PreparedEffect::new_with_factory(graph, rate, &mut |kind, rate| match kind {
        NodeKind::VoiceGroup { graph: text, voices } => {
            if scope.voice {
                return Err(DeviceSdkError("voice groups cannot nest through components".into()));
            }
            if *voices == 0 || *voices > 32 {
                return Err(DeviceSdkError("voice count must be 1..32".into()));
            }
            let child = crate::DeviceGraph::from_sd(text)?;
            if graph.role != crate::DeviceRole::Instrument
                || child.role != crate::DeviceRole::Instrument
                || child.nodes.iter().filter(|n| matches!(n.kind, NodeKind::VoiceOutput)).count() != 1
                || child.nodes.iter().any(|n| matches!(n.kind, NodeKind::VoiceGroup { .. }))
            {
                return Err(DeviceSdkError(
                    "VoiceGroup needs an instrument body with one VoiceOutput and no nested VoiceGroup".into(),
                ));
            }
            budget.voices = budget.voices.saturating_add(usize::from(*voices));
            if budget.voices > 128 {
                return Err(DeviceSdkError("expanded voice capacity exceeds 128".into()));
            }
            let mut instances = Vec::with_capacity(usize::from(*voices));
            for _ in 0..*voices {
                instances.push(compile_nested(
                    &child,
                    rate,
                    samples,
                    resolve,
                    Scope { depth: scope.depth + 1, voice: true, ..scope },
                    budget,
                )?);
            }
            Ok(Some(crate::nodes::voice::group(instances, rate)))
        }
        NodeKind::Component { graph: text, .. } => {
            let child = crate::DeviceGraph::from_sd(text)?;
            if graph.role == crate::DeviceRole::Effect && child.role == crate::DeviceRole::Instrument {
                return Err(DeviceSdkError("an effect component cannot hide an instrument".into()));
            }
            if child.nodes.iter().any(|n| matches!(n.kind, NodeKind::ComponentInput { index: 4 })) {
                return Err(DeviceSdkError("component input 4 is reserved for MIDI events".into()));
            }
            let prepared =
                compile_nested(&child, rate, samples, resolve, Scope { depth: scope.depth + 1, ..scope }, budget)?;
            Ok(Some(crate::nodes::component::prepared(prepared, rate)))
        }
        NodeKind::GrainPool { graph: text, grains } => {
            if scope.grain {
                return Err(DeviceSdkError("grain pools cannot nest through components".into()));
            }
            if *grains == 0 || *grains > 128 {
                return Err(DeviceSdkError("grain capacity must be 1..128".into()));
            }
            budget.grains = budget.grains.saturating_add(usize::from(*grains));
            if budget.grains > MAX_GRAINS_IN_ALL {
                return Err(DeviceSdkError("expanded grain capacity exceeds 256".into()));
            }
            let child = crate::DeviceGraph::from_sd(text)?;
            if child.role != crate::DeviceRole::Instrument
                || child.nodes.iter().filter(|n| matches!(n.kind, NodeKind::VoiceOutput)).count() != 1
                || child
                    .nodes
                    .iter()
                    .any(|n| matches!(n.kind, NodeKind::VoiceGroup { .. } | NodeKind::GrainPool { .. }))
            {
                return Err(DeviceSdkError("a grain body needs one VoiceOutput and no nested pools".into()));
            }
            let mut graphs = Vec::with_capacity(usize::from(*grains));
            for _ in 0..*grains {
                graphs.push(compile_nested(
                    &child,
                    rate,
                    samples,
                    resolve,
                    Scope { depth: scope.depth + 1, voice: true, grain: true },
                    budget,
                )?);
            }
            Ok(Some(crate::nodes::granular::pool(graphs, rate)))
        }
        NodeKind::GranularPlayer { sample, voices, grains, seed } => {
            let mut child = crate::nodes::granular::template(sample, *voices, *grains, *seed)?;
            for node in &mut child.nodes {
                let index = match node.id.as_str() {
                    "position" => Some(0),
                    "size" => Some(1),
                    "density" => Some(2),
                    "pitch" => Some(3),
                    _ => None,
                };
                if let Some(index) = index {
                    node.kind = NodeKind::ComponentInput { index };
                }
            }
            let graph =
                compile_nested(&child, rate, samples, resolve, Scope { depth: scope.depth + 1, ..scope }, budget)?;
            Ok(Some(crate::nodes::component::prepared(graph, rate)))
        }
        NodeKind::SamplerAsset { sample } => {
            let resource =
                resources.as_ref().ok_or_else(|| DeviceSdkError("missing sampler resources".into()))?.asset(sample)?;
            Ok(Some(crate::nodes::sample::prepare_bound(resource, rate)?))
        }
        NodeKind::TimeStretchPlayer { sample } => {
            let resource =
                resources.as_ref().ok_or_else(|| DeviceSdkError("missing stretch resources".into()))?.asset(sample)?;
            Ok(Some(crate::nodes::stretch::player(resource, rate)))
        }
        _ if kind.requires_samples() => crate::nodes::resources::prepare(
            kind,
            resources.as_ref().ok_or_else(|| DeviceSdkError("missing prepared resources".into()))?.clone(),
            rate,
        )
        .map(Some),
        _ => prepare_with(kind, rate, resolve),
    })
}
