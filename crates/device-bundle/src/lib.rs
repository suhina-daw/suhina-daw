// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Validated stock definitions embedded at build time and available without disk I/O.
use suhina_device_sdk::{DeviceGraph, DeviceSdkError};

pub struct StockDevice {
    pub id: &'static str,
    pub name: &'static str,
    pub instrument: bool,
    pub sd: &'static str,
}
include!(concat!(env!("OUT_DIR"), "/bundle.rs"));

/// Embedded project definitions take precedence over the shipped revision.
pub fn source(device: &suhina_model::Device) -> Option<&[u8]> {
    let suhina_model::DeviceKind::Builtin(id) = &device.kind else { return None };
    // A project stores a built-in by its id, never a copy of its definition (ADR 0021,
    // amendment 3): the panel is always the one this Suhina ships.
    find(id).map(|d| d.sd.as_bytes())
}

pub fn find(id: &str) -> Option<&'static StockDevice> {
    DEVICES.iter().find(|d| d.id == id)
}
impl StockDevice {
    pub fn graph(&self) -> Result<DeviceGraph, DeviceSdkError> {
        DeviceGraph::from_sd(self.sd)
    }
}
/// Used by the app's build script, never at startup. User-edited files in this build
/// output directory are generated resources and are replaced by the next build.
pub fn write_to(folder: &std::path::Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(folder)?;
    for device in DEVICES {
        let path = folder.join(format!("{}.sd", device.id));
        if std::fs::read(&path).ok().as_deref() != Some(device.sd.as_bytes()) {
            std::fs::write(path, device.sd)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_sampler_accepts_only_known_ordered_parameter_prefixes() {
        let mut graph = find("sampler").unwrap().graph().unwrap();
        graph.ui = None;
        graph.parameters.truncate(59);
        assert!(graph.prepare_native_at(48000., &suhina_device_registry::create).is_ok());
        let prepared = graph.prepare_native_at(48000., &suhina_device_registry::create).unwrap();
        for (index, p) in suhina_device_sdk::sampler::INFO.params.iter().enumerate().skip(59) {
            assert_eq!(prepared.param(index), Some(p.default));
        }
        graph.parameters.swap(0, 1);
        assert!(graph.prepare_native_at(48000., &suhina_device_registry::create).is_err());
        graph.parameters.swap(0, 1);
        graph.parameters.remove(5);
        assert!(graph.prepare_native_at(48000., &suhina_device_registry::create).is_err());
    }

    #[test]
    fn native_wrappers_reject_unknown_kernels_and_changed_contracts() {
        let stock = find("utility").unwrap();
        let mut graph = stock.graph().unwrap();
        assert!(graph.prepare_native_at(48000., &|_| None).is_err());
        graph.parameters[0].max += 1.;
        assert!(graph.prepare_native_at(48000., &suhina_device_registry::create).is_err());
        let mut graph = stock.graph().unwrap();
        graph.nodes.push(graph.nodes[0].clone());
        assert!(graph.native_device_id().is_err());
    }
    #[test]
    fn sampler_without_page_selector_still_prepares_but_audio_contract_changes_fail() {
        let mut graph = find("sampler").unwrap().graph().unwrap();
        graph.parameters.truncate(83);
        graph.ui = None;
        assert!(graph.prepare_native_at(48000., &suhina_device_registry::create).is_ok());
        graph.parameters.truncate(77);
        graph.ui = None;
        assert!(graph.prepare_native_at(48000., &suhina_device_registry::create).is_ok());
        graph.parameters.truncate(59);
        let playback = graph.parameters.iter_mut().find(|p| p.id == "playback").unwrap();
        playback.max = 2.;
        playback.ui.options.truncate(3);
        graph.ui = None;
        assert!(graph.prepare_native_at(48000., &suhina_device_registry::create).is_ok());
        graph.parameters.truncate(56);
        graph.ui = None;
        assert!(graph.prepare_native_at(48000., &suhina_device_registry::create).is_ok());
        graph.parameters.truncate(53);
        graph.ui = None;
        assert!(graph.prepare_native_at(48000., &suhina_device_registry::create).is_ok());
        graph.parameters.truncate(52);
        assert!(graph.prepare_native_at(48000., &suhina_device_registry::create).is_ok());
        graph.parameters[0].max += 1.;
        assert!(graph.prepare_native_at(48000., &suhina_device_registry::create).is_err());
        graph.parameters.pop();
        assert!(graph.prepare_native_at(48000., &suhina_device_registry::create).is_err());
    }
    #[test]
    fn sampler_layout_revision_preserves_values_and_adds_only_page_default() {
        let graph = find("sampler").unwrap().graph().unwrap();
        assert_eq!(graph.device_version, 12);
        let mut old: Vec<_> = graph
            .parameters
            .iter()
            .filter(|p| {
                !p.id.starts_with("slice_")
                    && p.id != "sensitivity"
                    && !matches!(
                        p.id.as_str(),
                        "panel_view"
                            | "lfo_filter"
                            | "lfo_volume"
                            | "lfo_pan"
                            | "lfo_shape"
                            | "fade_in"
                            | "fade_out"
                            | "warp"
                            | "warp_mode"
                            | "warp_beats"
                            | "lfo_rate_mode"
                            | "lfo_time"
                            | "lfo_division"
                            | "source_bpm"
                    )
            })
            .map(|p| (p.id.clone(), p.default))
            .collect();
        old.iter_mut().find(|p| p.0 == "gain").unwrap().1 = -12.;
        let migrated = graph.migrate_parameter_values(1, &old).unwrap();
        assert!(old.iter().all(|p| migrated.contains(p)));
        assert!(migrated.contains(&("panel_view".into(), 0.)));
        for id in ["lfo_filter", "lfo_volume", "lfo_pan", "lfo_shape", "fade_in", "fade_out"] {
            assert!(migrated.contains(&(id.into(), 0.)));
        }
        assert_eq!(migrated.len(), old.len() + 32);
        assert!(migrated.contains(&("warp".into(), 0.)));
        assert!(migrated.contains(&("warp_mode".into(), 1.)));
        assert!(migrated.contains(&("warp_beats".into(), 4.)));
        assert!(migrated.contains(&("source_bpm".into(), 0.)));
        assert!(migrated.contains(&("sensitivity".into(), 0.5)));
        assert!(migrated.contains(&("slice_by".into(), 0.)));
        assert!(migrated.contains(&("slice_1".into(), 1.)));
    }
    #[test]
    fn every_device_is_bundled_and_prepares_with_its_original_contract() {
        // Every built-in effect and instrument; MIDI effects have no stock wrapper yet.
        let bundled =
            suhina_device_registry::DEVICES.iter().filter(|info| suhina_device_registry::stock::bundled(info.id));
        assert_eq!(DEVICES.len(), bundled.count());
        for stock in DEVICES {
            let graph = stock.graph().unwrap();
            assert_eq!(graph.native_device_id().unwrap(), Some(stock.id));
            let native = graph.prepare_native_at(48000., &suhina_device_registry::create).unwrap();
            assert_eq!(native.info(), suhina_device_registry::info(stock.id).unwrap());
            assert!(graph.compile_at(48000.).is_err(), "a host factory is required");
        }
    }
}
