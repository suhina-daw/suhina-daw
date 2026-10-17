// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Commands for the plug-in library: scanning for installed plug-ins, listing them, and the
//! folders a scan looks in (ADR 0006). They belong to Suhina, not to one project, so they run
//! on [`Projects`] like the `projects.*` commands.

use std::path::Path;

use serde_json::{Value, json};
use suhina_model::{Device, DeviceKind};
use suhina_plugins::SavedState;
use suhina_plugins::matching::Match;
use suhina_plugins::scan::{Catalog, NO_PLUGINS, Report};

use crate::commands::{CommandKind, CommandSpec, Param, ParamType, optional, param};
use crate::projects::project_param;
use crate::{CommandError, Projects};

const FOLDER: Param = param("folder", ParamType::Text, "a folder that holds plug-ins");

static PLUGIN_COMMANDS: &[CommandSpec<Projects>] = &[
    CommandSpec {
        id: "plugins.list",
        label: "List the plug-ins the last scan found (id, name, vendor, version, features, file) and the files that failed; files that hold no plug-ins only with ignored",
        params: &[optional(
            "ignored",
            ParamType::Bool,
            "also list the files that hold no plug-ins, which scans pass over (default false)",
        )],
        kind: CommandKind::Plugins,
        run: list,
    },
    CommandSpec {
        id: "plugins.scan",
        label: "Look for installed plug-ins in the standard folders and the added ones; reads new and changed files and tries failed ones again (a hung one once a day), every file if all is true",
        params: &[optional("all", ParamType::Bool, "read every file again, failed ones too (default false)")],
        kind: CommandKind::Plugins,
        run: scan,
    },
    CommandSpec {
        id: "plugins.folders",
        label: "List the folders a scan looks in: the system's standard ones and the ones added",
        params: &[],
        kind: CommandKind::Plugins,
        run: folders,
    },
    CommandSpec {
        id: "plugins.addFolder",
        label: "Add a folder for scans to look in, besides the standard ones",
        params: &[FOLDER],
        kind: CommandKind::Plugins,
        run: add_folder,
    },
    CommandSpec {
        id: "plugins.removeFolder",
        label: "Stop looking in a folder that was added",
        params: &[FOLDER],
        kind: CommandKind::Plugins,
        run: remove_folder,
    },
    CommandSpec {
        id: "plugins.setHosting",
        label: "Choose how a plug-in's instances share host processes from their next start: binary (one process per plug-in file, the default), vendor (one per vendor) or instance (a process of its own for each)",
        params: &[
            param("plugin", ParamType::Text, "plug-in name, id or file, from plugins.list"),
            param("hosting", ParamType::Text, "binary, vendor or instance"),
        ],
        kind: CommandKind::Plugins,
        run: set_hosting,
    },
    CommandSpec {
        id: "plugins.check",
        label: "List a project's plug-in devices and the installed plug-in found for each: how it was found (same, declared, vst2-id, same-name, newer-version) and whether its saved settings are promised to carry over",
        params: &[optional("project", ParamType::Integer, "project id, from projects.list (default the active one)")],
        kind: CommandKind::Plugins,
        run: check,
    },
];

/// The commands for the plug-in library.
pub fn plugin_commands() -> &'static [CommandSpec<Projects>] {
    PLUGIN_COMMANDS
}

fn folder_param(params: &Value) -> Result<&Path, CommandError> {
    params
        .get("folder")
        .and_then(Value::as_str)
        .filter(|f| !f.is_empty())
        .map(Path::new)
        .ok_or_else(|| "folder must be a path".into())
}

fn paths(list: &[std::path::PathBuf]) -> Vec<String> {
    list.iter().map(|p| p.display().to_string()).collect()
}

fn list(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let ignored = bool_param(params, "ignored")?;
    let library = projects.library_mut();
    let specs: Vec<String> = library.catalog().plugins().map(|p| p.spec()).collect();
    let hosting: std::collections::HashMap<String, suhina_plugins::library::Hosting> =
        specs.into_iter().map(|spec| (spec.clone(), library.hosting(&spec))).collect();
    let catalog = projects.library_mut().catalog();
    let plugins: Vec<Value> = catalog
        .plugins()
        .map(|p| {
            json!({
                "format": p.format(),
                "id": p.id,
                "name": p.name,
                "vendor": p.vendor,
                "version": p.version,
                "features": p.features,
                "file": p.path.display().to_string(),
                "hosting": hosting.get(&p.spec()).copied().unwrap_or_default().name(),
            })
        })
        .collect();
    let failed: Vec<Value> =
        catalog.failed().map(|(file, why)| json!({ "file": file.display().to_string(), "reason": why })).collect();
    let mut listed = json!({ "plugins": plugins, "failed": failed });
    if ignored {
        listed["ignored"] =
            catalog.ignored().map(|file| json!({ "file": file.display().to_string(), "reason": NO_PLUGINS })).collect();
    }
    Ok(listed)
}

/// The optional true-or-false parameter `name` (default false).
fn bool_param(params: &Value, name: &str) -> Result<bool, CommandError> {
    let value = params.get(name).map(|v| v.as_bool().ok_or_else(|| format!("{name} must be true or false")));
    Ok(value.transpose()?.unwrap_or(false))
}

fn report(report: &Report, found: usize) -> Value {
    json!({
        "plugins": found,
        "read": paths(&report.read),
        "unchanged": report.unchanged,
        "removed": paths(&report.removed),
        "retried": paths(&report.retried),
        "ignored": paths(&report.ignored),
        "failed": report.failed.iter().map(|(file, why)| json!({ "file": file.display().to_string(), "reason": why })).collect::<Vec<_>>(),
    })
}

fn scan(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let all = bool_param(params, "all")?;
    let library = projects.library_mut();
    let done = library.scan(all)?;
    let found = library.catalog().plugins().count();
    Ok(report(&done, found))
}

fn folders(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    let library = projects.library_mut();
    let custom = paths(library.custom_folders());
    Ok(json!({ "standard": paths(&suhina_plugins::scan::standard_folders()), "added": custom }))
}

fn add_folder(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let added = projects.library_mut().add_folder(folder_param(params)?)?;
    Ok(json!({ "added": added, "folders": paths(projects.library_mut().custom_folders()) }))
}

fn set_hosting(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    use suhina_plugins::library::Hosting;
    let name = params.get("plugin").and_then(Value::as_str).ok_or("plugin must be a plug-in name, id or file")?;
    let hosting = params
        .get("hosting")
        .and_then(Value::as_str)
        .and_then(Hosting::parse)
        .ok_or("hosting must be binary, vendor or instance")?;
    let library = projects.library_mut();
    let catalog = library.catalog();
    let plugin = catalog
        .find(name)
        .or_else(|| catalog.plugins().find(|p| p.spec() == name))
        .ok_or_else(|| format!("no plug-in {name:?} in the catalog; plugins.list lists them"))?;
    let (spec, shown) = (plugin.spec(), plugin.name.clone());
    library.set_hosting(&spec, hosting)?;
    Ok(json!({ "plugin": shown, "hosting": hosting.name() }))
}

fn remove_folder(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let removed = projects.library_mut().remove_folder(folder_param(params)?)?;
    Ok(json!({ "removed": removed, "folders": paths(projects.library_mut().custom_folders()) }))
}

/// What the catalog has for one device: `None` for devices that are not plug-ins.
fn found<'a>(catalog: &'a Catalog, kind: &DeviceKind, name: &str) -> Option<Option<Match<'a>>> {
    match kind {
        DeviceKind::Vst3 { id } => Some(catalog.match_vst3(id, name)),
        DeviceKind::Vst2 { id } => Some(catalog.match_vst2(*id, name)),
        DeviceKind::Clap { id } => Some(catalog.match_clap(id, name)),
        DeviceKind::Builtin(_) | DeviceKind::Sd | DeviceKind::Foreign { .. } => None,
    }
}

/// The installed plug-in for a project's plug-in `device` and the state to give it (the saved
/// state, with a VST3 controller's own state when the project kept one); `None` when the device
/// is not a plug-in or nothing installed stands for it.
pub fn device_plugin<'a>(catalog: &'a Catalog, device: &Device) -> Option<(Match<'a>, Option<SavedState>)> {
    let found = found(catalog, &device.kind, &device.name)??;
    let controller = device.extensions.0.iter().find(|e| e.key == "Vst3ControllerState").map(|e| e.data.clone());
    let state = device.state.clone().map(|data| SavedState { data, controller });
    Some((found, state))
}

fn device_format(kind: &DeviceKind) -> (&'static str, String) {
    match kind {
        DeviceKind::Vst3 { id } => ("vst3", id.clone()),
        DeviceKind::Vst2 { id } => ("vst2", format!("{id:08X}")),
        DeviceKind::Clap { id } => ("clap", id.clone()),
        DeviceKind::Builtin(id) => ("builtin", id.clone()),
        DeviceKind::Sd => ("sd", String::new()),
        DeviceKind::Foreign { id, .. } => ("foreign", id.clone()),
    }
}

fn check(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let id = params.get("project").map(|_| project_param(params)).transpose()?;
    let devices: Vec<(String, suhina_model::Device)> = projects
        .session_mut(id)?
        .project()
        .channels
        .iter()
        .flat_map(|c| c.devices.iter().map(|d| (c.name.clone(), d.clone())))
        .collect();
    let catalog = projects.library_mut().catalog();
    let mut missing = 0;
    let list: Vec<Value> = devices
        .iter()
        .filter_map(|(channel, device)| {
            let result = found(catalog, &device.kind, &device.name)?;
            let (format, id) = device_format(&device.kind);
            let mut entry = json!({
                "channel": channel, "device": device.name, "format": format, "id": id,
                "has_state": device.state.is_some(),
            });
            match result {
                Some(m) => {
                    entry["found"] = json!({
                        "how": m.how.name(),
                        "keeps_state": m.how.keeps_state(),
                        "name": m.plugin.name,
                        "vendor": m.plugin.vendor,
                        "format": m.plugin.format(),
                        "plugin": m.plugin.spec(),
                    });
                }
                None => missing += 1,
            }
            Some(entry)
        })
        .collect();
    Ok(json!({ "plugins": list.len(), "missing": missing, "devices": list }))
}
