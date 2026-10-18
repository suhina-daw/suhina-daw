// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Append-only, off-thread graph authoring. Handles belong to one builder, never to the runtime.
use crate::{
    Connection, DeviceGraph, DeviceRole, DeviceSdkError, MAX_CONNECTIONS, MAX_NODES, MAX_PARAMETERS, MAX_TEXT_BYTES,
    Node, NodeKind, Parameter,
};
use std::sync::Arc;

/// A node in one builder. Cloning a handle does not create another DSP instance.
#[derive(Debug, Clone)]
pub struct NodeRef {
    owner: Arc<()>,
    index: usize,
}

#[derive(Debug, Clone)]
pub struct Output {
    node: NodeRef,
    port: String,
}

#[derive(Debug, Clone)]
pub struct Input {
    node: NodeRef,
    port: String,
}

impl NodeRef {
    pub fn output(&self, port: impl Into<String>) -> Output {
        Output { node: self.clone(), port: port.into() }
    }
    pub fn out(&self) -> Output {
        self.output("out")
    }
    pub fn left(&self) -> Output {
        self.output("left")
    }
    pub fn right(&self) -> Output {
        self.output("right")
    }
    pub fn input(&self, port: impl Into<String>) -> Input {
        Input { node: self.clone(), port: port.into() }
    }
}

/// Keeps node storage private so handles cannot become stale through deletion or reordering.
pub struct GraphBuilder {
    graph: DeviceGraph,
    owner: Arc<()>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_TEXT_BYTES
}

impl GraphBuilder {
    pub fn new(id: impl Into<String>, name: impl Into<String>, role: DeviceRole) -> Self {
        Self { graph: DeviceGraph::new(id, name, role), owner: Arc::new(()) }
    }

    pub fn node(&mut self, id: impl Into<String>, kind: NodeKind) -> Result<NodeRef, DeviceSdkError> {
        let id = id.into();
        if !valid_id(&id) || self.graph.nodes.iter().any(|node| node.id == id) {
            return Err(DeviceSdkError(format!("node {id:?}: id must be unique and 1..128 bytes")));
        }
        if self.graph.nodes.len() >= MAX_NODES {
            return Err(DeviceSdkError("node limit exceeded".into()));
        }
        let result = NodeRef { owner: self.owner.clone(), index: self.graph.nodes.len() };
        self.graph.nodes.push(Node::new(id, kind));
        Ok(result)
    }

    /// Declare a UI-only parameter. Validation of its value contract still happens in finish().
    pub fn parameter(&mut self, parameter: Parameter) -> Result<(), DeviceSdkError> {
        if !valid_id(&parameter.id) || self.graph.parameters.iter().any(|p| p.id == parameter.id) {
            return Err(DeviceSdkError(format!("parameter {:?}: id must be unique and 1..128 bytes", parameter.id)));
        }
        if self.graph.parameters.len() >= MAX_PARAMETERS {
            return Err(DeviceSdkError("parameter limit exceeded".into()));
        }
        self.graph.parameters.push(parameter);
        Ok(())
    }

    /// Declare a parameter and its source node, using the parameter id as the node id.
    pub fn parameter_signal(&mut self, parameter: Parameter) -> Result<NodeRef, DeviceSdkError> {
        let id = parameter.id.clone();
        // Insert both or neither, so a failed declaration is safe to correct and retry.
        self.parameter(parameter)?;
        match self.node(&id, NodeKind::Parameter { parameter: id.clone() }) {
            Ok(node) => Ok(node),
            Err(error) => {
                self.graph.parameters.pop();
                Err(error)
            }
        }
    }

    pub fn parameters(&self) -> &[Parameter] {
        &self.graph.parameters
    }

    fn resolve(&self, node: &NodeRef) -> Result<&Node, DeviceSdkError> {
        if !Arc::ptr_eq(&self.owner, &node.owner) {
            return Err(DeviceSdkError("node handle belongs to another graph".into()));
        }
        self.graph.nodes.get(node.index).ok_or_else(|| DeviceSdkError("invalid node handle".into()))
    }

    pub fn connect(&mut self, source: Output, destination: Input) -> Result<(), DeviceSdkError> {
        let from = self.resolve(&source.node)?;
        let to = self.resolve(&destination.node)?;
        if matches!(from.kind, NodeKind::AudioOutput | NodeKind::MidiOutput)
            || !from.kind.info().outputs.contains(&source.port.as_str())
        {
            return Err(DeviceSdkError(format!("node {:?}: invalid output port {:?}", from.id, source.port)));
        }
        if !to.kind.info().inputs.iter().any(|port| port.name == destination.port) {
            return Err(DeviceSdkError(format!("node {:?}: unknown input port {:?}", to.id, destination.port)));
        }
        if self.graph.connections.iter().any(|edge| edge.to == to.id && edge.to_port == destination.port) {
            return Err(DeviceSdkError(format!(
                "node {:?}, port {:?}: multiple connections; use add or mix",
                to.id, destination.port
            )));
        }
        if self.graph.connections.len() >= MAX_CONNECTIONS {
            return Err(DeviceSdkError("connection limit exceeded".into()));
        }
        let edge = Connection::ports(&from.id, source.port, &to.id, destination.port);
        self.graph.connections.push(edge);
        Ok(())
    }

    /// Insert a node and wire its inputs. An error leaves the builder unchanged.
    pub fn op(
        &mut self,
        id: impl Into<String>,
        kind: NodeKind,
        inputs: &[(&str, Output)],
    ) -> Result<NodeRef, DeviceSdkError> {
        let node = self.node(id, kind)?;
        let connections = self.graph.connections.len();
        for (port, source) in inputs {
            if let Err(error) = self.connect(source.clone(), node.input(*port)) {
                self.graph.connections.truncate(connections);
                self.graph.nodes.pop();
                return Err(error);
            }
        }
        Ok(node)
    }

    pub fn binary(
        &mut self,
        id: impl Into<String>,
        kind: NodeKind,
        a: Output,
        b: Output,
    ) -> Result<NodeRef, DeviceSdkError> {
        self.op(id, kind, &[("a", a), ("b", b)])
    }

    pub fn constant(&mut self, id: impl Into<String>, value: f64) -> Result<NodeRef, DeviceSdkError> {
        self.node(id, NodeKind::Constant { value })
    }

    /// Consume the builder, enforcing the same trust boundary as low-level graph authoring.
    pub fn finish(self) -> Result<DeviceGraph, DeviceSdkError> {
        self.graph.validate()?;
        Ok(self.graph)
    }
}
