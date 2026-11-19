// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Read-only import summary; never prints raw events or license metadata.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for path in std::env::args_os().skip(1) {
        let path = std::path::Path::new(&path);
        match suhina_flp::import_file(path) {
            Ok(result) => {
                println!(
                    "{}: {} tracks, {} contents, {} clips, {} warnings ({} missing local samples, {} gain envelopes)",
                    path.display(),
                    result.project.tracks.len(),
                    result.project.contents.len(),
                    result.project.arrangement.lanes.iter().map(|l| l.clips.len()).sum::<usize>(),
                    result.warnings.len(),
                    result.warnings.iter().filter(|w| w.starts_with("Sample not found")).count(),
                    result
                        .project
                        .arrangement
                        .lanes
                        .iter()
                        .flat_map(|l| &l.clips)
                        .filter(|c| !c.audio.gain_envelope.is_empty())
                        .count()
                );
                for channel in &result.project.channels {
                    if channel.id.0 >= 131_072 && channel.id.0 < 131_100 {
                        println!("  route {} -> {:?}, {} sends", channel.name, channel.output, channel.sends.len());
                    }
                    for (_, device) in suhina_model::walk_devices(&channel.devices) {
                        if let Some(group) = device.as_group() {
                            println!("  group {}: {} branches", channel.name, group.branches.len());
                            for branch in &group.branches {
                                println!("    branch {}: volume {}, muted {}", branch.name, branch.volume, branch.mute);
                            }
                        }
                        for lane in &device.automation {
                            println!("  automation {} / {}: {} points", channel.name, lane.param, lane.points.len());
                        }
                        if matches!(
                            device.kind,
                            suhina_model::DeviceKind::Vst2 { .. } | suhina_model::DeviceKind::Vst3 { .. }
                        ) {
                            println!(
                                "  plugin {}: {} {:?}, {} state bytes",
                                channel.name,
                                device.name,
                                device.kind,
                                device.state.as_deref().map_or(0, |s| s.len())
                            );
                        }
                    }
                }
                for warning in &result.warnings {
                    if warning.contains("Automation") || warning.contains("instrument identity") {
                        println!("  {warning}");
                    }
                }
                for lane in &result.project.arrangement.lanes {
                    if let Some(clip) = lane.clips.iter().find(|c| c.audio.warped) {
                        let name =
                            lane.track.and_then(|t| result.project.track(t)).map_or("Audio", |t| t.name.as_str());
                        if let Some(marker) = clip.audio.warp_markers.last() {
                            println!(
                                "  {name}: {} source seconds -> {} beats, {:?}, trim {} beats",
                                marker.audio_time, marker.time, clip.audio.stretch, clip.offset
                            );
                        }
                    }
                }
            }
            Err(e) => println!("{}: {e}", path.display()),
        }
    }
    Ok(())
}
