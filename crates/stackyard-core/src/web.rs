//! What serves each site: the web server's site list, and one PHP process
//! per PHP version in use.
//!
//! - PHP sites go through FastCGI: `php-fpm` on macOS and Linux, `php-cgi`
//!   on Windows (the official Windows builds have no php-fpm).
//! - Node sites are proxied to the app's own dev server.
//! - Static sites are served straight from the folder.
//!
//! Every site also answers on `<name>.localhost`, which browsers send to
//! this computer on their own, so sites work before local domains are set up.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::{self, RuntimeKind};
use crate::detect::{self, Needs};
use crate::platform::{self, Os};
use crate::runtimes::RuntimeManager;
use crate::services::{slash, write_config};
use crate::settings::{read_toml, write_toml, Settings};
use crate::shell::pins;
use crate::sites::{Site, SiteRuntime, SiteStore};
use crate::supervisor::ProcessSpec;
use crate::{domain, ports, Error, Result};

/// The web server's ports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ports {
    pub http: u16,
    pub https: u16,
}

impl Ports {
    pub const STANDARD: Ports = Ports {
        http: 80,
        https: 443,
    };
    /// Used when the standard ports are taken or not allowed (Linux keeps
    /// ports below 1024 for administrators until system setup lowers that).
    pub const FALLBACK: Ports = Ports {
        http: 8080,
        https: 8443,
    };

    /// The standard ports when this computer lets Stackyard use them.
    /// Only meaningful while the web server is stopped.
    pub fn choose() -> Ports {
        if ports::is_free(80) && ports::is_free(443) {
            Ports::STANDARD
        } else {
            Ports::FALLBACK
        }
    }

    /// The ports the web server last started with, saved by [`Ports::save`].
    pub fn current(home: &Path) -> Ports {
        read_toml(&ports_file(home))
            .ok()
            .flatten()
            .unwrap_or(Ports::STANDARD)
    }

    pub fn save(self, home: &Path) -> Result<()> {
        write_toml(&ports_file(home), &self)
    }

    /// `https://myapp.stack`, with the port only when it is not the usual one.
    pub fn url(self, domain: &str, https: bool) -> String {
        let (scheme, port, usual) = if https {
            ("https", self.https, 443)
        } else {
            ("http", self.http, 80)
        };
        if port == usual {
            format!("{scheme}://{domain}")
        } else {
            format!("{scheme}://{domain}:{port}")
        }
    }
}

fn ports_file(home: &Path) -> PathBuf {
    home.join("config").join("caddy").join("ports.toml")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase", tag = "kind")]
pub enum Backend {
    Php {
        version: String,
        /// The FastCGI port of that version's PHP process.
        port: u16,
        installed: bool,
    },
    Node {
        version: String,
        /// The app's dev server.
        port: u16,
    },
    Static,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServedSite {
    #[serde(flatten)]
    pub site: Site,
    /// Found in a parked folder rather than added by hand.
    pub parked: bool,
    pub framework: Option<&'static str>,
    /// The folder the web server serves.
    pub root: PathBuf,
    pub backend: Backend,
}

impl ServedSite {
    pub fn domain(&self) -> String {
        self.site.domain()
    }

    /// The `.localhost` name that works without any system setup.
    pub fn localhost(&self) -> String {
        format!("{}.localhost", self.site.name)
    }
}

/// Whether `url` is a page on this computer: `127.0.0.1`, `localhost`, a
/// `.localhost` name or a name under one of `endings` (e.g. `.stack`), over
/// http or https, with nothing in it a shell or browser could misread.
pub fn is_local_url(url: &str, endings: &[String]) -> bool {
    let safe = url
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b":/.-_?=&%#".contains(&b));
    let Some(rest) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
    else {
        return false;
    };
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = host_port.split_once(':').unwrap_or((host_port, ""));
    let host = host.to_ascii_lowercase();
    let port_ok = port.is_empty() || port.parse::<u16>().is_ok();
    let local = host == "127.0.0.1"
        || host == "localhost"
        || host.ends_with(".localhost")
        || endings.iter().any(|e| {
            let e = e.trim_start_matches('.').to_ascii_lowercase();
            !e.is_empty() && host.ends_with(&format!(".{e}"))
        });
    safe && port_ok && local
}

/// The FastCGI port for a PHP version: 8.4 → 9084.
pub fn php_port(version: &str) -> u16 {
    let (major, minor) = version.split_once('.').unwrap_or((version, "0"));
    let major: u16 = major.parse().unwrap_or(0);
    let minor: u16 = minor.parse().unwrap_or(0);
    9000 + major.min(9) * 10 + minor.min(9)
}

/// Every site the web server should serve: the ones added by hand first,
/// then one per project folder inside each parked folder.
pub fn sites(
    home: &Path,
    settings: &Settings,
    runtimes: &RuntimeManager,
) -> Result<Vec<ServedSite>> {
    let store = SiteStore::load_from(&home.join("sites.toml"))?;
    let mut served: Vec<ServedSite> = store
        .list()
        .iter()
        .map(|site| from_site(site.clone(), false, runtimes))
        .collect();
    let mut taken: BTreeSet<String> = served.iter().map(ServedSite::domain).collect();
    for folder in &settings.parked {
        for (name, path) in project_folders(folder) {
            let site = parked_site(name, path, settings, runtimes);
            if taken.insert(site.domain()) {
                served.push(from_site(site, true, runtimes));
            }
        }
    }
    Ok(served)
}

fn from_site(site: Site, parked: bool, runtimes: &RuntimeManager) -> ServedSite {
    let detected = detect::detect(&site.path);
    let backend = match &site.runtime {
        SiteRuntime::Php { version } => Backend::Php {
            version: version.clone(),
            port: php_port(version),
            installed: runtimes.get(RuntimeKind::Php, version).is_some(),
        },
        SiteRuntime::Node { version, port } => Backend::Node {
            version: version.clone(),
            port: *port,
        },
        SiteRuntime::Static => Backend::Static,
    };
    // A framework's public folder applies to PHP and plain files alike.
    let root = match detected.needs {
        Needs::Node { .. } => site.path.clone(),
        _ => site.path.join(&detected.root),
    };
    ServedSite {
        site,
        parked,
        framework: detected.framework,
        root,
        backend,
    }
}

/// Project folders inside a parked folder, as (site name, path), by name.
fn project_folders(parked: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(parked) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let file_name = e.file_name();
            let file_name = file_name.to_str()?;
            if file_name.starts_with('.') {
                return None;
            }
            Some((site_name(file_name)?, e.path()))
        })
        .collect();
    found.sort();
    found
}

/// `My Shop_v2` → `my-shop-v2`; `None` when nothing usable is left.
pub fn site_name(folder: &str) -> Option<String> {
    let mut name = String::new();
    for c in folder.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            name.push(c);
        } else if !name.ends_with('-') {
            name.push('-');
        }
    }
    let name: String = name.trim_matches('-').chars().take(63).collect();
    let name = name.trim_end_matches('-').to_string();
    domain::validate_site_name(&name).ok().map(|_| name)
}

fn parked_site(
    name: String,
    path: PathBuf,
    settings: &Settings,
    runtimes: &RuntimeManager,
) -> Site {
    Site {
        name,
        tld: settings.tld.clone(),
        runtime: runtime_for(&path, settings, runtimes),
        path,
        https: true,
    }
}

/// The runtime a project folder needs, with the default versions.
pub fn runtime_for(path: &Path, settings: &Settings, runtimes: &RuntimeManager) -> SiteRuntime {
    // A version pinned in the project's `.stackyard` file, else the default.
    let version = |kind| match pins::pinned(path, kind) {
        Ok(Some(pin)) => pin.version,
        _ => default_version(settings, runtimes, kind),
    };
    match detect::detect(path).needs {
        Needs::Php => SiteRuntime::Php {
            version: version(RuntimeKind::Php),
        },
        Needs::Node { port } => SiteRuntime::Node {
            version: version(RuntimeKind::Node),
            port,
        },
        Needs::Static => SiteRuntime::Static,
    }
}

/// The default version, else the newest installed, else the newest offered.
pub fn default_version(
    settings: &Settings,
    runtimes: &RuntimeManager,
    kind: RuntimeKind,
) -> String {
    if let Some(installed) = runtimes.default_for(settings, kind) {
        return installed.version;
    }
    let versions = catalog::runtime(kind).versions;
    versions
        .iter()
        .find(|v| runtimes.get(kind, v).is_some())
        .or(versions.first())
        .map_or_else(String::new, |v| v.to_string())
}

/// Caddyfile site blocks for every site.
pub fn caddy_sites(sites: &[ServedSite]) -> String {
    let mut out = String::new();
    for served in sites {
        let scheme = if served.site.https { "" } else { "http://" };
        out.push_str(&format!(
            "\n# {}\n{scheme}{}, {scheme}{} {{\n",
            served.site.path.display(),
            served.domain(),
            served.localhost()
        ));
        match &served.backend {
            Backend::Php {
                version,
                installed: false,
                ..
            } => out.push_str(&format!(
                "\trespond \"PHP {version} is not installed. Install it in Stackyard under Runtimes.\" 503\n"
            )),
            Backend::Php { port, .. } => {
                out.push_str(&format!("\troot * {}\n", quote(&slash(&served.root))));
                out.push_str("\tencode zstd gzip\n");
                out.push_str(&format!("\tphp_fastcgi 127.0.0.1:{port}\n"));
                out.push_str("\tfile_server\n");
            }
            Backend::Node { port, .. } => {
                // Dev servers often refuse unknown host names, so pass theirs.
                out.push_str(&format!(
                    "\treverse_proxy localhost:{port} {{\n\t\theader_up Host {{upstream_hostport}}\n\t}}\n"
                ));
            }
            Backend::Static => {
                out.push_str(&format!("\troot * {}\n", quote(&slash(&served.root))));
                out.push_str("\tencode zstd gzip\n");
                out.push_str("\tfile_server\n");
            }
        }
        out.push_str("}\n");
    }
    out
}

/// A Caddyfile string token.
fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// One process per installed PHP version that a site uses.
pub fn php_processes(
    home: &Path,
    sites: &[ServedSite],
    runtimes: &RuntimeManager,
) -> Result<Vec<ProcessSpec>> {
    let versions: BTreeSet<&str> = sites
        .iter()
        .filter_map(|s| match &s.backend {
            Backend::Php {
                version,
                installed: true,
                ..
            } => Some(version.as_str()),
            _ => None,
        })
        .collect();
    let os = runtimes.platform().os;
    versions
        .into_iter()
        .filter_map(|v| runtimes.get(RuntimeKind::Php, v))
        .map(|installed| php_process(home, &installed.version, &installed.path, os))
        .collect()
}

/// The process id for a PHP version, e.g. `php-8.4`.
pub fn php_process_id(version: &str) -> String {
    format!("php-{version}")
}

fn php_process(home: &Path, version: &str, install: &Path, os: Os) -> Result<ProcessSpec> {
    let config = home.join("config").join("php").join(version);
    std::fs::create_dir_all(&config).map_err(|source| Error::Io {
        path: config.clone(),
        source,
    })?;
    let ini = config.join("php.ini");
    if !ini.exists() {
        write_config(&ini, &php_ini(install, os))?;
    }
    let port = php_port(version);
    let (program, args, env) = if os == Os::Windows {
        (
            install.join("php-cgi.exe"),
            vec![
                "-b".to_string(),
                format!("127.0.0.1:{port}"),
                "-c".to_string(),
                slash(&ini),
            ],
            // Serve requests until stopped instead of exiting after 500.
            vec![("PHP_FCGI_MAX_REQUESTS".to_string(), "0".to_string())],
        )
    } else {
        let fpm = config.join("php-fpm.conf");
        write_config(&fpm, &fpm_conf(port))?;
        let mut args = vec![
            "--nodaemonize".to_string(),
            "--fpm-config".to_string(),
            slash(&fpm),
            "-c".to_string(),
            slash(&ini),
        ];
        // php-fpm refuses to run as root unless told to (containers, CI).
        if platform::running_as_root() {
            args.push("--allow-to-run-as-root".into());
        }
        (install.join("bin/php-fpm"), args, Vec::new())
    };
    Ok(ProcessSpec {
        id: php_process_id(version),
        name: format!("PHP {version}"),
        program,
        args,
        env,
        cwd: Some(config),
        ports: vec![port],
        restart_on_crash: true,
    })
}

/// Development defaults. Written once; the user's edits are kept.
fn php_ini(install: &Path, os: Os) -> String {
    let mut ini = String::from(
        "; Created by Stackyard with development defaults. Your edits are kept.\n\
         memory_limit = 512M\n\
         upload_max_filesize = 128M\n\
         post_max_size = 128M\n\
         max_execution_time = 120\n\
         display_errors = On\n\
         display_startup_errors = On\n\
         error_reporting = E_ALL\n\
         log_errors = On\n\
         date.timezone = UTC\n",
    );
    if os == Os::Windows {
        // The Windows builds ship extensions as separate files; turn on the
        // ones most projects need, where this version has them.
        let ext = install.join("ext");
        ini.push_str(&format!("extension_dir = {}\n", quote(&slash(&ext))));
        for name in [
            "bz2",
            "curl",
            "fileinfo",
            "gd",
            "gd2",
            "gmp",
            "intl",
            "mbstring",
            "exif",
            "mysqli",
            "openssl",
            "pdo_mysql",
            "pdo_pgsql",
            "pdo_sqlite",
            "pgsql",
            "sodium",
            "sqlite3",
            "zip",
        ] {
            if ext.join(format!("php_{name}.dll")).is_file() {
                ini.push_str(&format!("extension={name}\n"));
            }
        }
        if ext.join("php_opcache.dll").is_file() {
            ini.push_str("zend_extension=opcache\n");
        }
    }
    ini
}

fn fpm_conf(port: u16) -> String {
    format!(
        "; Written by Stackyard before each start; edits here are replaced.\n\
         [global]\n\
         error_log = /dev/stderr\n\
         daemonize = no\n\
         \n\
         [stackyard]\n\
         listen = 127.0.0.1:{port}\n\
         pm = ondemand\n\
         pm.max_children = 10\n\
         pm.process_idle_timeout = 60s\n\
         clear_env = no\n\
         catch_workers_output = yes\n\
         decorate_workers_output = no\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn knows_local_urls() {
        let endings = [".stack".to_string()];
        for url in [
            "http://127.0.0.1:8025",
            "http://localhost",
            "https://myapp.stack/",
            "https://myapp.stack:8443/admin?x=1",
            "http://shop.localhost:8080",
        ] {
            assert!(is_local_url(url, &endings), "{url}");
        }
        for url in [
            "https://example.com",
            "https://stack",
            "file:///etc/passwd",
            "http://localhost.evil.com",
            "https://a.stack\"&calc",
            "http://localhost:99999",
            "javascript:alert(1)",
        ] {
            assert!(!is_local_url(url, &endings), "{url}");
        }
    }

    #[test]
    fn php_ports_follow_the_version() {
        assert_eq!(php_port("8.4"), 9084);
        assert_eq!(php_port("7.4"), 9074);
        assert_eq!(php_port("8.10"), 9089);
    }

    #[test]
    fn urls_skip_the_usual_ports() {
        assert_eq!(Ports::STANDARD.url("a.stack", true), "https://a.stack");
        assert_eq!(Ports::STANDARD.url("a.stack", false), "http://a.stack");
        assert_eq!(Ports::FALLBACK.url("a.stack", true), "https://a.stack:8443");
        let home = crate::testutil::tempdir();
        assert_eq!(Ports::current(&home), Ports::STANDARD);
        Ports::FALLBACK.save(&home).unwrap();
        assert_eq!(Ports::current(&home), Ports::FALLBACK);
    }

    #[test]
    fn names_sites_after_folders() {
        assert_eq!(site_name("My Shop_v2").as_deref(), Some("my-shop-v2"));
        assert_eq!(site_name("--api--").as_deref(), Some("api"));
        assert_eq!(site_name("ü"), None);
        assert_eq!(site_name(&"a".repeat(80)).unwrap().len(), 63);
    }

    #[test]
    fn parked_folders_become_sites() {
        let home = crate::testutil::tempdir();
        let code = home.join("Code");
        touch(&code.join("blog/artisan"));
        touch(&code.join("blog/public/index.php"));
        touch(&code.join("Landing Page/index.html"));
        touch(&code.join(".hidden/index.html"));
        touch(&code.join("notes.txt"));
        std::fs::create_dir_all(code.join("shop")).unwrap();
        std::fs::write(
            code.join("shop/package.json"),
            r#"{"scripts": {"dev": "vite"}, "devDependencies": {"vite": "7"}}"#,
        )
        .unwrap();

        // A site added by hand keeps its name over a parked folder.
        let mut store = SiteStore::load_from(&home.join("sites.toml")).unwrap();
        store
            .add(Site {
                name: "shop".into(),
                tld: ".stack".into(),
                path: home.join("elsewhere"),
                runtime: SiteRuntime::Static,
                https: false,
            })
            .unwrap();
        store.save().unwrap();

        let mut settings = Settings::default();
        settings.park(&code).unwrap();
        let runtimes = RuntimeManager::new(&home);
        let served = sites(&home, &settings, &runtimes).unwrap();
        let names: Vec<_> = served.iter().map(|s| s.domain()).collect();
        assert_eq!(names, ["shop.stack", "blog.stack", "landing-page.stack"]);

        let blog = &served[1];
        assert!(blog.parked);
        assert_eq!(blog.framework, Some("Laravel"));
        assert_eq!(blog.root, code.join("blog/public"));
        assert_eq!(
            blog.backend,
            Backend::Php {
                version: "8.5".into(),
                port: 9085,
                installed: false
            }
        );
        assert_eq!(served[2].backend, Backend::Static);
        assert!(!served[0].parked);

        // A `.stackyard` file in the project picks its version.
        std::fs::write(code.join("blog/.stackyard"), "php = \"8.2\"\n").unwrap();
        let served = sites(&home, &settings, &runtimes).unwrap();
        assert!(matches!(&served[1].backend, Backend::Php { version, .. } if version == "8.2"));
    }

    #[test]
    fn writes_site_blocks() {
        let served = |runtime, https| ServedSite {
            site: Site {
                name: "app".into(),
                tld: ".stack".into(),
                path: PathBuf::from("/code/app"),
                runtime,
                https,
            },
            parked: false,
            framework: None,
            root: PathBuf::from("/code/app/public"),
            backend: Backend::Static,
        };
        let mut php = served(
            SiteRuntime::Php {
                version: "8.4".into(),
            },
            true,
        );
        php.backend = Backend::Php {
            version: "8.4".into(),
            port: 9084,
            installed: true,
        };
        let text = caddy_sites(&[php.clone()]);
        assert!(text.contains("app.stack, app.localhost {"), "{text}");
        assert!(text.contains("root * \"/code/app/public\""), "{text}");
        assert!(text.contains("php_fastcgi 127.0.0.1:9084"), "{text}");

        php.backend = Backend::Php {
            version: "8.4".into(),
            port: 9084,
            installed: false,
        };
        assert!(caddy_sites(&[php]).contains("not installed"));

        let mut node = served(
            SiteRuntime::Node {
                version: "22".into(),
                port: 5173,
            },
            false,
        );
        node.backend = Backend::Node {
            version: "22".into(),
            port: 5173,
        };
        let text = caddy_sites(&[node]);
        assert!(
            text.contains("http://app.stack, http://app.localhost {"),
            "{text}"
        );
        assert!(text.contains("reverse_proxy localhost:5173"), "{text}");
        assert_eq!(quote("C:/a \"b\""), "\"C:/a \\\"b\\\"\"");
    }

    #[test]
    fn writes_php_settings_once() {
        let home = crate::testutil::tempdir();
        let install = home.join("runtimes/php/8.4");
        let spec = php_process(&home, "8.4", &install, Os::Linux).unwrap();
        assert_eq!(spec.id, "php-8.4");
        assert_eq!(spec.ports, vec![9084]);
        assert!(spec.program.ends_with("bin/php-fpm"));
        let fpm = std::fs::read_to_string(home.join("config/php/8.4/php-fpm.conf")).unwrap();
        assert!(fpm.contains("listen = 127.0.0.1:9084"));

        let ini = home.join("config/php/8.4/php.ini");
        std::fs::write(&ini, "memory_limit = 1G\n").unwrap();
        php_process(&home, "8.4", &install, Os::Linux).unwrap();
        assert_eq!(
            std::fs::read_to_string(&ini).unwrap(),
            "memory_limit = 1G\n"
        );

        touch(&install.join("ext/php_mbstring.dll"));
        let windows = php_process(&home, "8.3", &install, Os::Windows).unwrap();
        assert!(windows.program.ends_with("php-cgi.exe"));
        let ini = std::fs::read_to_string(home.join("config/php/8.3/php.ini")).unwrap();
        assert!(ini.contains("extension=mbstring"), "{ini}");
        assert!(!ini.contains("extension=curl"), "{ini}");
    }
}
