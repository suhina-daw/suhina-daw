// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Commands for the plug-in library: scanning for installed plug-ins, listing them, switching
//! them on and off, and the folders a scan looks in (ADR 0006). They belong to Suhina, not to
//! one project, so they run on [`Projects`] like the `project.*` (on the open projects) commands.

use std::path::Path;

use serde_json::{Value, json};
use suhina_model::{Device, DeviceKind};
use suhina_plugins::SavedState;
use suhina_plugins::matching::Match;
use suhina_plugins::scan::{Catalog, FolderFormat, NO_PLUGINS, Report};

use crate::commands::{CommandKind, CommandSpec, Param, ParamType, optional, param};
use crate::projects::project_param;
use crate::{Agents, CommandError, PathKind, Policy, Projects};

const FOLDER: Param = param("folder", ParamType::Path(PathKind::Folder), "a folder that holds plug-ins");
const PLUGIN: Param = param("plugin", ParamType::Text, "plug-in name, id or spec, from plugin.list");
const ENABLED: Param = param("enabled", ParamType::Bool, "true for on, false for off");

static PLUGIN_COMMANDS: &[CommandSpec<Projects>] = &[
    CommandSpec {
        id: "plugin.list",
        label: "List the plug-ins the last scan found (id, name, vendor, type, version, features, whether it supports ARA, file, whether it is switched on), the files that failed, and those built for another processor; files that hold no plug-ins only with ignored",
        params: &[optional(
            "ignored",
            ParamType::Bool,
            "also list the files that hold no plug-ins, which scans pass over (default false)",
        )],
        kind: CommandKind::Plugins,
        run: list,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "plugin.scan",
        label: "Look for installed plug-ins in the standard folders and the added ones; reads new and changed files and tries failed ones again (a hung one once a day), every file if all is true",
        params: &[optional("all", ParamType::Bool, "read every file again, failed ones too (default false)")],
        kind: CommandKind::Plugins,
        run: scan,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "plugin.folders",
        label: "List the folders a scan looks in: the system's standard ones and the ones added, each with its format and whether it is switched on",
        params: &[],
        kind: CommandKind::Plugins,
        run: folders,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "plugin.addFolder",
        label: "Add a folder for scans to look in, besides the standard ones, for one plug-in format or every one Suhina hosts",
        params: &[
            FOLDER,
            optional("format", ParamType::Text, "any (default: every format Suhina hosts), vst3, clap or vst2"),
        ],
        kind: CommandKind::Plugins,
        run: add_folder,
        policy: Policy {
            agents: Agents::Denied("the user chooses plug-in folders (suhina-cli plugins add-folder)"),
            ..Policy::DEFAULT
        },
    },
    CommandSpec {
        id: "plugin.setFolderEnabled",
        label: "Switch a plug-in folder (standard or added) on or off for scans; the plug-ins of a folder switched off leave the list with the next scan",
        params: &[param("folder", ParamType::Text, "a folder the user added, as plugin.folders lists them"), ENABLED],
        kind: CommandKind::Plugins,
        run: set_folder_enabled,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "plugin.setEnabled",
        label: "Switch an installed plug-in on or off: one switched off is not offered in the browser and does not load, and a project that uses it finds it missing",
        params: &[PLUGIN, ENABLED],
        kind: CommandKind::Plugins,
        run: set_enabled,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "plugin.scanFile",
        label: "Read one plug-in file of the list again now, a failed one too",
        params: &[param("file", ParamType::Text, "the plug-in file, as plugin.list gives it")],
        kind: CommandKind::Plugins,
        run: scan_file,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "plugin.removeFolder",
        label: "Stop looking in a folder that was added",
        params: &[FOLDER],
        kind: CommandKind::Plugins,
        run: remove_folder,
        policy: Policy {
            agents: Agents::Denied("the user chooses plug-in folders (suhina-cli plugins add-folder)"),
            ..Policy::DEFAULT
        },
    },
    CommandSpec {
        id: "plugin.setHosting",
        label: "Choose how a plug-in's instances share host processes from their next start: binary (one process per plug-in file, the default), vendor (one per vendor) or instance (a process of its own for each; not for a plug-in that edits clips over ARA)",
        params: &[
            param("plugin", ParamType::Text, "plug-in name, id or file, from plugin.list"),
            param("hosting", ParamType::Text, "binary, vendor or instance"),
        ],
        kind: CommandKind::Plugins,
        run: set_hosting,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "plugin.check",
        label: "List a project's plug-in devices and the installed plug-in found for each: how it was found (same, declared, vst2-id, same-name, newer-version) and whether its saved settings are promised to carry over",
        params: &[optional("project", ParamType::Integer, "project id, from project.list (default the active one)")],
        kind: CommandKind::Plugins,
        run: check,
        policy: Policy::DEFAULT,
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
    let off: std::collections::HashSet<String> =
        hosting.keys().filter(|spec| !library.is_enabled(spec)).cloned().collect();
    let catalog = projects.library_mut().catalog();
    let plugins: Vec<Value> = catalog
        .plugins()
        .map(|p| {
            json!({
                "format": p.format(),
                "id": p.id,
                "name": p.name,
                "vendor": p.vendor,
                "type": if crate::is_instrument(p) { "instrument" } else { "effect" },
                "version": p.version,
                "features": p.features,
                "ara": p.ara,
                "file": p.path.display().to_string(),
                "hosting": hosting.get(&p.spec()).copied().unwrap_or_default().name(),
                "enabled": !off.contains(&p.spec()),
            })
        })
        .collect();
    let failed = file_reasons(catalog.failed());
    let incompatible = file_reasons(catalog.incompatible());
    let mut listed = json!({ "plugins": plugins, "failed": failed, "incompatible": incompatible });
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
        "failed": reasons(&report.failed),
        "incompatible": reasons(&report.incompatible),
    })
}

/// Files with why each gave no plug-ins, as `plugin.list` lists them.
fn file_reasons<'a>(list: impl Iterator<Item = (&'a Path, &'a str)>) -> Vec<Value> {
    list.map(|(file, why)| json!({ "file": file.display().to_string(), "reason": why })).collect()
}

/// Files with why each gave no plug-ins, as the reports list them.
fn reasons(list: &[(std::path::PathBuf, String)]) -> Vec<Value> {
    list.iter().map(|(file, why)| json!({ "file": file.display().to_string(), "reason": why })).collect()
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
    let custom = paths(&library.custom_folders());
    let every: Vec<Value> = library
        .folder_list()
        .into_iter()
        .map(|f| {
            json!({
                "folder": f.path.display().to_string(),
                "format": f.format.name(),
                "enabled": f.enabled,
                "standard": f.standard,
            })
        })
        .collect();
    Ok(json!({ "standard": paths(&suhina_plugins::scan::standard_folders()), "added": custom, "folders": every }))
}

fn add_folder(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let format = match params.get("format") {
        None => FolderFormat::Any,
        Some(v) => v.as_str().and_then(FolderFormat::parse).ok_or("format must be any, vst3, clap or vst2")?,
    };
    let added = projects.library_mut().add_folder_as(folder_param(params)?, format)?;
    Ok(json!({ "added": added, "folders": paths(&projects.library_mut().custom_folders()) }))
}

fn set_folder_enabled(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let enabled = params.get("enabled").and_then(Value::as_bool).ok_or("enabled must be true or false")?;
    let changed = projects.library_mut().set_folder_enabled(folder_param(params)?, enabled)?;
    Ok(json!({ "changed": changed, "enabled": enabled }))
}

fn set_enabled(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let name = params.get("plugin").and_then(Value::as_str).ok_or("plugin must be a plug-in name, id or spec")?;
    let enabled = params.get("enabled").and_then(Value::as_bool).ok_or("enabled must be true or false")?;
    let library = projects.library_mut();
    let catalog = library.catalog();
    let plugin = catalog
        .plugins()
        .find(|p| p.spec() == name)
        .or_else(|| catalog.find(name))
        .ok_or_else(|| format!("no plug-in {name:?} in the catalog; plugin.list lists them"))?;
    let (spec, shown) = (plugin.spec(), plugin.name.clone());
    library.set_enabled(&spec, enabled)?;
    Ok(json!({ "plugin": shown, "spec": spec, "enabled": enabled }))
}

fn scan_file(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let file = params.get("file").and_then(Value::as_str).filter(|f| !f.is_empty()).ok_or("file must be a path")?;
    let library = projects.library_mut();
    let done = library.scan_file_again(Path::new(file))?;
    let found = library.catalog().plugins().count();
    Ok(report(&done, found))
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
        .ok_or_else(|| format!("no plug-in {name:?} in the catalog; plugin.list lists them"))?;
    if hosting == Hosting::Instance && plugin.ara {
        return Err(format!(
            "{} edits clips over ARA, so its instances share one process with the document they edit; \
             choose binary or vendor",
            plugin.name
        )
        .into());
    }
    let (spec, shown) = (plugin.spec(), plugin.name.clone());
    library.set_hosting(&spec, hosting)?;
    Ok(json!({ "plugin": shown, "hosting": hosting.name() }))
}

fn remove_folder(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let removed = projects.library_mut().remove_folder(folder_param(params)?)?;
    Ok(json!({ "removed": removed, "folders": paths(&projects.library_mut().custom_folders()) }))
}

/// What the catalog has for one device: `None` for devices that are not plug-ins.
fn found<'a>(catalog: &'a Catalog, kind: &DeviceKind, name: &str) -> Option<Option<Match<'a>>> {
    match kind {
        DeviceKind::Vst3 { id } => Some(catalog.match_vst3(id, name)),
        DeviceKind::Vst2 { id } => Some(catalog.match_vst2(*id, name)),
        DeviceKind::Clap { id } => Some(catalog.match_clap(id, name)),
        DeviceKind::Builtin(_) | DeviceKind::Sd | DeviceKind::Foreign { .. } | DeviceKind::Group(_) => None,
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
        DeviceKind::Group(_) => ("group", String::new()),
    }
}

fn check(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let id = params.get("project").map(|_| project_param(params)).transpose()?;
    let devices: Vec<(String, suhina_model::Device)> = projects
        .session_mut(id)?
        .project()
        .channels
        .iter()
        // Plug-ins inside device groups too.
        .flat_map(|c| suhina_model::walk_devices(&c.devices).into_iter().map(|(_, d)| (c.name.clone(), d.clone())))
        .collect();
    // What sessions load: a plug-in switched off is missing to a project that uses it.
    let enabled = projects.library_mut().enabled_catalog();
    let catalog = &enabled;
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
