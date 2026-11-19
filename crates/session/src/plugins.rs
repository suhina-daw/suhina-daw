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

use crate::commands::{Args, CommandKind, CommandSpec, Param, ParamType, optional, param};
use crate::projects::project_param;
use crate::{Agents, CommandError, PathKind, Policy, Projects};

const FOLDER: Param = param("folder", ParamType::Path(PathKind::Folder), "a folder that holds plug-ins");
const ENABLED: Param = param("enabled", ParamType::Bool, "true for on, false for off");
/// One plug-in, or with [`PLUGINS`] a list of them saved at once.
const PLUGIN_ONE: Param =
    optional("plugin", ParamType::Text, "plug-in name, id or spec, from plugin.list (or plugins)");
const PLUGINS: Param =
    optional("plugins", ParamType::Texts, "several plug-ins (names, ids or specs), changed and saved at once");

static PLUGIN_COMMANDS: &[CommandSpec<Projects>] = &[
    CommandSpec {
        id: "plugin.list",
        label: "List the plug-ins the last scan found (id, name, vendor, type, version, features, whether it supports ARA, file, whether it is switched on, whether it is marked as owned), the files that failed, and those built for another processor; files that hold no plug-ins only with ignored",
        params: &[optional(
            "ignored",
            ParamType::Bool,
            "also list the files that hold no plug-ins, which scans pass over (default false)",
        )],
        kind: CommandKind::on(crate::commands::Target::Plugins, crate::commands::Effect::Read),
        run: list,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "plugin.scan",
        label: "Look for installed plug-ins in the standard folders and the added ones; reads new and changed files and tries failed ones again (a hung one once a day), every file if all is true",
        params: &[optional("all", ParamType::Bool, "read every file again, failed ones too (default false)")],
        kind: CommandKind::on(crate::commands::Target::Plugins, crate::commands::Effect::Change),
        run: scan,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "plugin.folders",
        label: "List the folders a scan looks in: the system's standard ones and the ones added, each with its format and whether it is switched on",
        params: &[],
        kind: CommandKind::on(crate::commands::Target::Plugins, crate::commands::Effect::Read),
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
        kind: CommandKind::on(crate::commands::Target::Plugins, crate::commands::Effect::Change),
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
        kind: CommandKind::on(crate::commands::Target::Plugins, crate::commands::Effect::Change),
        run: set_folder_enabled,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "plugin.setEnabled",
        label: "Switch installed plug-ins on or off, one or a list: one switched off is not offered in the browser and does not load, and a project that uses it finds it missing",
        params: &[PLUGIN_ONE, PLUGINS, ENABLED],
        kind: CommandKind::on(crate::commands::Target::Plugins, crate::commands::Effect::Change),
        run: set_enabled,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "plugin.setOwned",
        label: "Mark an installed plug-in as owned, or take the mark away: a mark for the user's own plug-ins among a vendor's bundle that installs them all, naming the product (vendor and name), so every format of it is owned; what is offered and loaded does not change",
        params: &[
            PLUGIN_ONE,
            PLUGINS,
            param("owned", ParamType::Bool, "true to mark it as owned, false to take the mark away"),
        ],
        kind: CommandKind::on(crate::commands::Target::Plugins, crate::commands::Effect::Change),
        run: set_owned,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "plugin.scanFile",
        label: "Read one plug-in file of the list again now, a failed one too",
        params: &[param("file", ParamType::Text, "the plug-in file, as plugin.list gives it")],
        kind: CommandKind::on(crate::commands::Target::Plugins, crate::commands::Effect::Change),
        run: scan_file,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "plugin.removeFolder",
        label: "Stop looking in a folder that was added",
        params: &[FOLDER],
        kind: CommandKind::on(crate::commands::Target::Plugins, crate::commands::Effect::Change),
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
        kind: CommandKind::on(crate::commands::Target::Plugins, crate::commands::Effect::Change),
        run: set_hosting,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "plugin.check",
        label: "List a project's plug-in devices and the installed plug-in found for each: how it was found (same, declared, vst2-id, same-name, newer-version) and whether its saved settings are promised to carry over",
        params: &[optional("project", ParamType::Integer, "project id, from project.list (default the active one)")],
        kind: CommandKind::on(crate::commands::Target::Plugins, crate::commands::Effect::Read),
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
    let ignored = Args::new(params).flag("ignored")?;
    let library = projects.library_mut();
    let specs: Vec<String> = library.catalog().plugins().map(|p| p.spec()).collect();
    let hosting: std::collections::HashMap<String, suhina_plugins::library::Hosting> =
        specs.into_iter().map(|spec| (spec.clone(), library.hosting(&spec))).collect();
    let off: std::collections::HashSet<String> =
        hosting.keys().filter(|spec| !library.is_enabled(spec)).cloned().collect();
    // A mark names the product (vendor and name), so every format of it is owned.
    let marks: std::collections::HashSet<String> = library.owned_keys().into_iter().collect();
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
                "owned": marks.contains(&suhina_plugins::library::product_key(&p.vendor, &p.name)),
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
    let all = Args::new(params).flag("all")?;
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
    let format = match Args::new(params).value("format") {
        None => FolderFormat::Any,
        Some(v) => v.as_str().and_then(FolderFormat::parse).ok_or("format must be any, vst3, clap or vst2")?,
    };
    let added = projects.library_mut().add_folder_as(folder_param(params)?, format)?;
    Ok(json!({ "added": added, "folders": paths(&projects.library_mut().custom_folders()) }))
}

fn set_folder_enabled(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let enabled = Args::new(params).bool("enabled")?;
    let changed = projects.library_mut().set_folder_enabled(folder_param(params)?, enabled)?;
    Ok(json!({ "changed": changed, "enabled": enabled }))
}

/// The plug-ins a switch or a mark acts on: `plugin`, one name, id or spec, or `plugins`, a list
/// of them (one save for them all). Each found as `plugin.list` names it; the first not found is
/// an error and nothing changes.
fn named_plugins(
    projects: &mut Projects,
    params: &Value,
) -> Result<Vec<(String, String, String, String)>, CommandError> {
    let names: Vec<&str> = match (params.get("plugin"), params.get("plugins")) {
        (Some(one), None) => vec![one.as_str().ok_or("plugin must be a plug-in name, id or spec")?],
        (None, Some(Value::Array(list))) => list
            .iter()
            .map(|v| v.as_str().ok_or("plugins must be a list of plug-in names, ids or specs"))
            .collect::<Result<_, _>>()?,
        (None, Some(_)) => return Err("plugins must be a list of plug-in names, ids or specs".into()),
        _ => return Err("give plugin (one plug-in) or plugins (a list), not both".into()),
    };
    if names.len() > 4096 {
        return Err("at most 4096 plug-ins at a time".into());
    }
    let catalog = projects.library_mut().catalog();
    // Each spec worked out once, not once per name looked up.
    let by_spec: std::collections::HashMap<String, &suhina_plugins::scan::PluginInfo> =
        catalog.plugins().map(|p| (p.spec(), p)).collect();
    names
        .into_iter()
        .map(|name| {
            let plugin = by_spec
                .get(name)
                .copied()
                .or_else(|| catalog.find(name))
                .ok_or_else(|| format!("no plug-in {name:?} in the catalog; plugin.list lists them"))?;
            Ok((plugin.spec(), plugin.name.clone(), plugin.vendor.clone(), name.to_string()))
        })
        .collect()
}

/// What a switch or a mark answers: the plug-in for `plugin`, the list for `plugins`.
fn answer(params: &Value, found: &[(String, String, String, String)], field: &str, value: bool) -> Value {
    match (params.get("plugin"), found) {
        (Some(_), [(spec, name, _, _)]) => json!({ "plugin": name, "spec": spec, field: value }),
        _ => json!({ "plugins": found.iter().map(|(_, name, _, _)| name.as_str()).collect::<Vec<_>>(), field: value }),
    }
}

fn set_enabled(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let enabled = Args::new(params).bool("enabled")?;
    let found = named_plugins(projects, params)?;
    let specs: Vec<String> = found.iter().map(|(spec, ..)| spec.clone()).collect();
    projects.library_mut().set_enabled_many(&specs, enabled)?;
    Ok(answer(params, &found, "enabled", enabled))
}

fn set_owned(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    use suhina_plugins::library::product_key;
    let owned = Args::new(params).bool("owned")?;
    // A mark of a plug-in no longer installed is taken away by its name, so it never sticks.
    if !owned
        && let Some(name) = params.get("plugin").and_then(Value::as_str)
        && projects.library_mut().catalog().find(name).is_none()
        && !projects.library_mut().catalog().plugins().any(|p| p.spec() == name)
    {
        let keys: Vec<String> = projects
            .library_mut()
            .owned_keys()
            .into_iter()
            .filter(|key| key.split_once('\t').is_some_and(|(_, n)| n.eq_ignore_ascii_case(name)) || key == name)
            .collect();
        if keys.is_empty() {
            return Err(format!("no plug-in {name:?} in the catalog or marked as owned; plugin.list lists them").into());
        }
        projects.library_mut().set_owned_many(&keys, false)?;
        return Ok(json!({ "plugin": name, "owned": false }));
    }
    let found = named_plugins(projects, params)?;
    let keys: Vec<String> = found.iter().map(|(_, name, vendor, _)| product_key(vendor, name)).collect();
    projects.library_mut().set_owned_many(&keys, owned)?;
    Ok(answer(params, &found, "owned", owned))
}

fn scan_file(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let file = Args::new(params).raw_str("file").filter(|f| !f.is_empty()).ok_or("file must be a path")?;
    let library = projects.library_mut();
    let done = library.scan_file_again(Path::new(file))?;
    let found = library.catalog().plugins().count();
    Ok(report(&done, found))
}

fn set_hosting(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    use suhina_plugins::library::Hosting;
    let name = Args::new(params).raw_str("plugin").ok_or("plugin must be a plug-in name, id or file")?;
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
    let found = found(catalog, &device.kind, device.plugin_name())??;
    let controller = device.extensions.0.iter().find(|e| e.key == "Vst3ControllerState").map(|e| e.data.clone());
    let state = device.state.as_deref().map(|data| SavedState { data: data.to_vec(), controller });
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
    let id = Args::new(params).value("project").map(|_| project_param(params)).transpose()?;
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
            let result = found(catalog, &device.kind, device.plugin_name())?;
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
