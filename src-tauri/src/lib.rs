//! Tauri commands the React interface calls. They are thin wrappers over
//! `stackyard-core`; errors come back to the interface as plain strings.

use serde::Serialize;
use stackyard_core::catalog::{RuntimeFamily, ServiceDef, RUNTIMES, SERVICES};
use stackyard_core::modules::{ModuleDef, MODULES};
use stackyard_core::settings::Settings;
use stackyard_core::sites::{Site, SiteRuntime, SiteStore};
use stackyard_core::supervisor::{ProcessState, Supervisor};
use stackyard_core::{domain, paths};
use tauri::{Manager, RunEvent, State};

type CmdResult<T> = Result<T, String>;

#[derive(Serialize)]
struct SiteView {
    name: String,
    domain: String,
    url: String,
    path: String,
    runtime: String,
    https: bool,
}

impl From<&Site> for SiteView {
    fn from(site: &Site) -> Self {
        let runtime = match &site.runtime {
            SiteRuntime::Php { version } => format!("PHP {version}"),
            SiteRuntime::Node { version, .. } => format!("Node {version}"),
            SiteRuntime::Static => "Static".to_string(),
        };
        Self {
            name: site.name.clone(),
            domain: site.domain(),
            url: site.url(),
            path: site.path.display().to_string(),
            runtime,
            https: site.https,
        }
    }
}

#[derive(Serialize)]
struct TldView {
    tld: String,
    warning: Option<&'static str>,
}

fn err(e: stackyard_core::Error) -> String {
    e.to_string()
}

#[tauri::command]
fn list_runtimes() -> &'static [RuntimeFamily] {
    RUNTIMES
}

#[tauri::command]
fn list_services() -> &'static [ServiceDef] {
    SERVICES
}

#[tauri::command]
fn get_tld() -> CmdResult<TldView> {
    let tld = Settings::load().map_err(err)?.tld;
    Ok(TldView {
        warning: domain::tld_warning(&tld),
        tld,
    })
}

#[tauri::command]
fn set_tld(value: String) -> CmdResult<TldView> {
    let mut settings = Settings::load().map_err(err)?;
    settings.set_tld(&value).map_err(err)?;
    settings.save().map_err(err)?;
    Ok(TldView {
        warning: domain::tld_warning(&settings.tld),
        tld: settings.tld,
    })
}

#[derive(Serialize)]
struct ModuleView {
    #[serde(flatten)]
    def: &'static ModuleDef,
    enabled: bool,
}

fn module_views(settings: &Settings) -> Vec<ModuleView> {
    MODULES
        .iter()
        .map(|def| ModuleView {
            def,
            enabled: settings.module_enabled(def.id),
        })
        .collect()
}

#[tauri::command]
fn list_modules() -> CmdResult<Vec<ModuleView>> {
    Ok(module_views(&Settings::load().map_err(err)?))
}

#[tauri::command]
fn set_module(
    id: String,
    enabled: bool,
    supervisor: State<'_, Supervisor>,
) -> CmdResult<Vec<ModuleView>> {
    let mut settings = Settings::load().map_err(err)?;
    settings.set_module(&id, enabled).map_err(err)?;
    settings.save().map_err(err)?;
    if !enabled && supervisor.state(&id) != ProcessState::Stopped {
        supervisor.stop(&id).map_err(err)?;
    }
    Ok(module_views(&settings))
}

#[tauri::command]
fn service_states(supervisor: State<'_, Supervisor>) -> Vec<(String, ProcessState)> {
    supervisor.states()
}

#[tauri::command]
fn list_sites() -> CmdResult<Vec<SiteView>> {
    let store = SiteStore::load().map_err(err)?;
    Ok(store.list().iter().map(SiteView::from).collect())
}

#[tauri::command]
fn add_site(name: String, path: String, runtime: String, https: bool) -> CmdResult<SiteView> {
    let settings = Settings::load().map_err(err)?;
    let runtime = parse_runtime(&runtime)?;
    let mut store = SiteStore::load().map_err(err)?;
    let view = SiteView::from(
        store
            .add(Site {
                name,
                tld: settings.tld,
                path: path.into(),
                runtime,
                https,
            })
            .map_err(err)?,
    );
    store.save().map_err(err)?;
    Ok(view)
}

#[tauri::command]
fn remove_site(name: String) -> CmdResult<()> {
    let mut store = SiteStore::load().map_err(err)?;
    store.remove(&name).map_err(err)?;
    store.save().map_err(err)
}

/// Accepts the labels the Sites screen offers: `PHP 8.3`, `Node 22`, `Static`.
fn parse_runtime(label: &str) -> CmdResult<SiteRuntime> {
    match label.split_once(' ') {
        Some(("PHP", version)) => Ok(SiteRuntime::Php {
            version: version.to_string(),
        }),
        Some(("Node", version)) => Ok(SiteRuntime::Node {
            version: version.to_string(),
            port: 3000,
        }),
        None if label == "Static" => Ok(SiteRuntime::Static),
        _ => Err(format!("unknown runtime {label:?}")),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(Supervisor::new(paths::logs_dir()))
        .invoke_handler(tauri::generate_handler![
            list_runtimes,
            list_services,
            list_modules,
            set_module,
            service_states,
            get_tld,
            set_tld,
            list_sites,
            add_site,
            remove_site
        ])
        .build(tauri::generate_context!())
        .expect("error while building Stackyard");
    app.run(|handle, event| {
        // Never leave services running after Stackyard quits.
        if let RunEvent::Exit = event {
            handle.state::<Supervisor>().stop_all();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_labels() {
        assert_eq!(
            parse_runtime("PHP 8.3").unwrap(),
            SiteRuntime::Php {
                version: "8.3".into()
            }
        );
        assert!(matches!(
            parse_runtime("Node 22").unwrap(),
            SiteRuntime::Node { .. }
        ));
        assert_eq!(parse_runtime("Static").unwrap(), SiteRuntime::Static);
        assert!(parse_runtime("Ruby 3").is_err());
    }
}
