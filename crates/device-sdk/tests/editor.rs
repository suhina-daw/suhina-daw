// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_sdk::{Connection, DeviceGraph, editor::*};

#[path = "../examples/wire_layout.rs"]
mod example;

#[test]
fn layout_roundtrips_and_wire_identity_survives_reordering() {
    let mut g = example::build();
    g.connections.reverse();
    g.nodes.reverse();
    let text = g.to_sd().unwrap();
    assert_eq!(DeviceGraph::from_sd(&text).unwrap(), g);
    assert!(text.contains("\"version\": 5"));
    for version in [1, 2] {
        assert!(DeviceGraph::from_sd(&text.replace("\"version\": 5", &format!("\"version\": {version}"))).is_err());
    }
    assert!(DeviceGraph::from_sd(&text.replace("\"graph\":", "\"unsupported\":")).is_err());
    assert!(DeviceGraph::from_sd(&text.replace("\"points\":", "\"unsupported\":")).is_err());
}

#[test]
fn old_files_load_and_missing_layout_fields_default() {
    let mut json: serde_json::Value = serde_json::from_str(&example::build().to_sd().unwrap()).unwrap();
    json.as_object_mut().unwrap().remove("editor");
    json["version"] = 2.into();
    assert!(DeviceGraph::from_sd(&json.to_string()).unwrap().editor.is_none());
    let v1 = DeviceGraph::from_sd(include_str!("fixtures/gain-v1.sd")).unwrap();
    assert!(v1.editor.is_none());
    assert_eq!(DeviceGraph::from_sd(&v1.to_sd().unwrap()).unwrap(), v1);
    json["version"] = 3.into();
    json["editor"] = serde_json::json!({"graph":{"wires":[{"connection":{"from":"in","to":"gain"}}]}});
    let graph = DeviceGraph::from_sd(&json.to_string()).unwrap();
    let layout = graph.editor.unwrap().graph;
    assert!(layout.nodes.is_empty());
    assert!(layout.wires[0].points.is_empty());
    assert!(layout.wires[0].from_anchor.is_none());
}

#[test]
fn editor_metadata_does_not_change_audio_or_block_size_behavior() {
    let g = example::build();
    let mut plain = g.clone();
    plain.editor = None;
    for block in [2, 14, 128] {
        let mut with = g.compile().unwrap();
        let mut without = plain.compile().unwrap();
        let mut a = vec![0.25; 512];
        let mut b = a.clone();
        for (a, b) in a.chunks_mut(block).zip(b.chunks_mut(block)) {
            with.process(a);
            without.process(b);
        }
        assert_eq!(a, b);
        assert_eq!(a, vec![0.25; 512]);
    }
}

#[test]
fn invalid_references_duplicates_and_hostile_geometry_are_rejected() {
    fn rejects(change: impl FnOnce(&mut GraphLayout)) {
        let mut g = example::build();
        change(&mut g.editor.as_mut().unwrap().graph);
        assert!(g.validate().is_err(), "{g:?}");
    }
    rejects(|l| l.nodes[0].node = "missing".into());
    rejects(|l| l.nodes.push(l.nodes[0].clone()));
    rejects(|l| l.wires.push(l.wires[0].clone()));
    rejects(|l| l.wires[0].connection.to = "missing".into());
    rejects(|l| l.wires[0].connection.from_port = "left".into());
    rejects(|l| {
        l.nodes.remove(0);
    });
    for value in [f32::NAN, f32::INFINITY, -8193.0, 8193.0] {
        rejects(|l| l.nodes[0].position[0] = value);
        rejects(|l| l.wires[0].points[0][0] = value);
    }
    for value in [f32::NAN, f32::INFINITY, -1.0, 0.0, 4097.0] {
        rejects(|l| l.nodes[0].size[1] = value);
    }
    for value in [f32::NAN, f32::INFINITY, -1.0, 121.0] {
        rejects(|l| l.wires[0].from_anchor = Some([value, 40.0]));
    }
    rejects(|l| l.wires[0].points = vec![[0.0; 2]; MAX_WIRE_POINTS + 1]);
    rejects(|l| l.nodes = vec![l.nodes[0].clone(); suhina_device_sdk::MAX_NODES + 1]);
    rejects(|l| l.wires = vec![l.wires[0].clone(); suhina_device_sdk::MAX_CONNECTIONS + 1]);
}

#[test]
fn fanout_anchors_must_agree_and_total_bends_are_bounded() {
    use suhina_device_sdk::{DeviceRole, Node, NodeKind};
    let mut g = DeviceGraph::new("org.test.fanout", "Fanout", DeviceRole::Effect);
    g.nodes = vec![Node::audio_input("in"), Node::new("sum", NodeKind::Add), Node::audio_output("out")];
    g.connections = vec![
        Connection::ports("in", "out", "sum", "a"),
        Connection::ports("in", "out", "sum", "b"),
        Connection::new("sum", "out"),
    ];
    g.editor = Some(Editor {
        graph: GraphLayout {
            nodes: vec![NodeLayout { node: "in".into(), position: [-20.0, -30.0], size: [120.0, 80.0] }],
            wires: g
                .connections
                .iter()
                .take(2)
                .map(|c| WireLayout {
                    connection: c.clone(),
                    points: vec![],
                    from_anchor: Some([120.0, 40.0]),
                    to_anchor: None,
                })
                .collect(),
        },
    });
    g.validate().unwrap();
    g.editor.as_mut().unwrap().graph.wires[1].from_anchor = Some([120.0, 20.0]);
    assert!(g.validate().is_err());

    let mut g = example::build();
    g.connections.clear();
    g.nodes = vec![Node::audio_input("in")];
    for i in 0..18 {
        let from = if i == 0 { "in".to_owned() } else { format!("n{}", i - 1) };
        let to = format!("n{i}");
        g.nodes.push(Node::new(&to, NodeKind::Tanh));
        g.connections.push(Connection::new(from, to));
    }
    g.nodes.push(Node::audio_output("out"));
    g.connections.push(Connection::new("n17", "out"));
    g.editor = Some(Editor {
        graph: GraphLayout {
            nodes: vec![],
            wires: g
                .connections
                .iter()
                .map(|c| WireLayout {
                    connection: c.clone(),
                    points: vec![[0.0; 2]; MAX_WIRE_POINTS],
                    from_anchor: None,
                    to_anchor: None,
                })
                .collect(),
        },
    });
    assert!(g.validate().unwrap_err().0.contains("bends"));
    g.editor.as_mut().unwrap().graph.wires.truncate(MAX_TOTAL_WIRE_POINTS / MAX_WIRE_POINTS);
    g.validate().unwrap();
}
