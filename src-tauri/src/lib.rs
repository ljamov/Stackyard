//! Tauri commands the React interface calls. They are thin wrappers over
//! `stackyard-core`; errors come back to the interface as plain strings.

use serde::Serialize;
use stackyard_core::catalog::{RuntimeFamily, ServiceDef, RUNTIMES, SERVICES};
use stackyard_core::domain;
use stackyard_core::settings::Settings;
use stackyard_core::sites::{Site, SiteRuntime, SiteStore};

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
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_runtimes,
            list_services,
            get_tld,
            set_tld,
            list_sites,
            add_site,
            remove_site
        ])
        .run(tauri::generate_context!())
        .expect("error while running Stackyard");
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
