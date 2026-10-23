//! Public links to a local site, for showing work to someone elsewhere.
//!
//! Uses Cloudflare's free quick tunnels: `cloudflared tunnel --url …` asks
//! trycloudflare.com for a random `https://<words>.trycloudflare.com`
//! address and passes visits to it on to this computer. No account is
//! needed. cloudflared is Cloudflare's own program (Apache 2.0), downloaded
//! from its GitHub releases the first time a link is made and checked
//! against the SHA-256 GitHub records for it.
//!
//! Quick tunnels are for testing: Cloudflare promises no uptime, and their
//! terms of use apply. Each link lasts until it is stopped or Stackyard
//! quits, and anyone with it can open the site.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::download::Http;
use crate::install::{self, Step};
use crate::platform::{Arch, Os, Platform};
use crate::services::{github, ServiceManager};
use crate::sites::Site;
use crate::supervisor::{ProcessSpec, ProcessState, Supervisor};
use crate::web::Ports;
use crate::{Error, Result};

const REPO: &str = "cloudflare/cloudflared";
/// How long cloudflared gets to report its address.
const WAIT: Duration = Duration::from_secs(40);

pub fn dir(home: &Path) -> PathBuf {
    home.join("tools").join("cloudflared")
}

fn program_name(os: Os) -> &'static str {
    if os == Os::Windows {
        "cloudflared.exe"
    } else {
        "cloudflared"
    }
}

/// Stackyard's cloudflared, once downloaded.
pub fn program(home: &Path) -> Option<PathBuf> {
    let path = dir(home).join(program_name(Platform::current().os));
    path.is_file().then_some(path)
}

/// The installed version, e.g. `2026.10.1`.
pub fn version(home: &Path) -> Option<String> {
    program(home)?;
    install::read_receipt(&dir(home)).map(|r| r.release)
}

/// The release file for a system: a plain program, or a `.tgz` on macOS.
fn asset(platform: Platform) -> Option<&'static str> {
    Some(match (platform.os, platform.arch) {
        (Os::Windows, Arch::X64) => "cloudflared-windows-amd64.exe",
        (Os::Linux, Arch::X64) => "cloudflared-linux-amd64",
        (Os::Linux, Arch::Arm64) => "cloudflared-linux-arm64",
        (Os::Macos, Arch::X64) => "cloudflared-darwin-amd64.tgz",
        (Os::Macos, Arch::Arm64) => "cloudflared-darwin-arm64.tgz",
        _ => return None,
    })
}

/// Downloads the latest cloudflared into `tools/cloudflared`. Returns the version.
pub fn install(home: &Path, progress: &mut dyn FnMut(Step)) -> Result<String> {
    let platform = Platform::current();
    let name = asset(platform).ok_or_else(|| Error::NotOnThisSystem {
        name: "cloudflared".into(),
        system: platform.to_string(),
    })?;
    progress(Step::Resolving);
    let http = Http::new();
    let latest = github::latest(&http, REPO)?;
    let release = github::release(&latest, "cloudflared", &[name.to_string()], platform)?;
    install::install(
        &http,
        &home.join("downloads"),
        &release,
        &dir(home),
        program_name(platform.os),
        progress,
    )?;
    Ok(release.version)
}

pub fn uninstall(home: &Path) -> Result<()> {
    if dir(home).exists() {
        install::remove(&dir(home))?;
    }
    Ok(())
}

/// The process id of a site's link, e.g. `share-blog`.
pub fn process_id(site: &Site) -> String {
    format!("share-{}", site.key())
}

/// The cloudflared command for a site: visits go to the local web server
/// with the site's own name, so it is served like any visit on this
/// computer.
pub fn spec(home: &Path, program: &Path, site: &Site) -> ProcessSpec {
    let ports = Ports::current(home);
    let domain = site.domain();
    let mut args: Vec<String> = vec![
        "tunnel".into(),
        // An empty settings file, so a `~/.cloudflared/config.yml` the person
        // has for their own tunnels is not picked up.
        "--config".into(),
        config_file(home).display().to_string(),
        "--no-autoupdate".into(),
        "--http-host-header".into(),
        domain.clone(),
    ];
    if site.https {
        args.extend([
            "--url".into(),
            format!("https://127.0.0.1:{}", ports.https),
            "--origin-server-name".into(),
            domain,
        ]);
        let root = ServiceManager::new(home).root_certificate();
        if root.is_file() {
            args.extend(["--origin-ca-pool".into(), root.display().to_string()]);
        } else {
            // Only the hop to this computer's own web server is unchecked.
            args.push("--no-tls-verify".into());
        }
    } else {
        args.extend(["--url".into(), format!("http://127.0.0.1:{}", ports.http)]);
    }
    ProcessSpec {
        id: process_id(site),
        name: format!("Public link to {}", site.domain()),
        program: program.to_owned(),
        args,
        env: Vec::new(),
        cwd: Some(dir(home)),
        ports: Vec::new(),
        // A new start gets a new address, which the person would not know.
        restart_on_crash: false,
    }
}

fn config_file(home: &Path) -> PathBuf {
    dir(home).join("empty-config.yml")
}

/// The `https://….trycloudflare.com` address in cloudflared's output.
pub fn find_url(lines: &[String]) -> Option<String> {
    lines.iter().rev().find_map(|line| {
        let start = line.find("https://")?;
        let url: String = line[start..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '.' | '-'))
            .collect();
        let host = url.strip_prefix("https://")?;
        let name = host.strip_suffix(".trycloudflare.com")?;
        (!name.is_empty() && name != "api" && !name.contains('.')).then_some(url)
    })
}

/// A site's public link, while it is up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    pub site: String,
    /// `None` while cloudflared is still connecting.
    pub url: Option<String>,
}

/// Starts a public link for `site` and waits for its address.
pub fn start(home: &Path, supervisor: &Supervisor, site: &Site) -> Result<Link> {
    let program =
        program(home).ok_or_else(|| Error::Shell("cloudflared is not downloaded yet".into()))?;
    let config = config_file(home);
    std::fs::write(&config, "# Stackyard's quick tunnels need no settings.\n").map_err(
        |source| Error::Io {
            path: config.clone(),
            source,
        },
    )?;
    let id = process_id(site);
    if !matches!(supervisor.state(&id), ProcessState::Running { .. }) {
        // A fresh log, so the address found is this run's.
        let _ = std::fs::remove_file(supervisor.log_path(&id));
        supervisor.start(spec(home, &program, site))?;
    }
    let started = Instant::now();
    loop {
        let lines = supervisor.tail_log(&id, 200).unwrap_or_default();
        if let Some(url) = find_url(&lines) {
            return Ok(Link {
                site: site.domain(),
                url: Some(url),
            });
        }
        if !matches!(supervisor.state(&id), ProcessState::Running { .. }) {
            let reason = lines
                .iter()
                .rev()
                .find(|l| l.contains("ERR") || l.starts_with("failed"))
                .cloned()
                .unwrap_or_else(|| "cloudflared stopped".into());
            return Err(Error::Shell(format!(
                "could not make a public link: {reason}"
            )));
        }
        if started.elapsed() > WAIT {
            let _ = supervisor.stop(&id);
            return Err(Error::Shell(
                "could not make a public link: Cloudflare did not answer in time; check your internet connection and try again".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

pub fn stop(supervisor: &Supervisor, site: &Site) -> Result<()> {
    let id = process_id(site);
    if supervisor.state(&id) != ProcessState::Stopped {
        supervisor.stop(&id)?;
    }
    Ok(())
}

/// Every link that is up, by site domain.
pub fn links(supervisor: &Supervisor, sites: &[Site]) -> Vec<Link> {
    sites
        .iter()
        .filter_map(|site| {
            let id = process_id(site);
            matches!(supervisor.state(&id), ProcessState::Running { .. }).then(|| Link {
                site: site.domain(),
                url: find_url(&supervisor.tail_log(&id, 200).unwrap_or_default()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sites::SiteRuntime;

    fn site(https: bool) -> Site {
        Site {
            name: "blog".into(),
            tld: ".stack".into(),
            path: PathBuf::from("/code/blog"),
            runtime: SiteRuntime::Static,
            https,
            dev_server: false,
            settings: Default::default(),
        }
    }

    #[test]
    fn finds_the_address_in_the_output() {
        let lines: Vec<String> = [
            "2026-10-17T16:57:35Z INF Requesting new quick Tunnel on trycloudflare.com...",
            "2026-10-17T16:57:36Z INF +--------------------------------------------------------------------------------------------+",
            "2026-10-17T16:57:36Z INF |  Your quick Tunnel has been created! Visit it at (it may take some time to be reachable):  |",
            "2026-10-17T16:57:36Z INF |  https://seasonal-tiger-hold-example.trycloudflare.com                                     |",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            find_url(&lines).as_deref(),
            Some("https://seasonal-tiger-hold-example.trycloudflare.com")
        );
        let failed: Vec<String> = [
            "failed to request quick Tunnel: Post \"https://api.trycloudflare.com/tunnel\": dial tcp",
            "see https://www.cloudflare.com/website-terms/",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(find_url(&failed), None);
    }

    #[test]
    fn sends_visits_to_the_local_web_server_by_name() {
        let home = crate::testutil::tempdir();
        let spec = spec(&home, Path::new("/x/cloudflared"), &site(true));
        assert_eq!(spec.id, "share-blog");
        let args = spec.args.join(" ");
        assert!(args.contains("--url https://127.0.0.1:443"), "{args}");
        assert!(args.contains("--http-host-header blog.stack"), "{args}");
        assert!(args.contains("--origin-server-name blog.stack"), "{args}");
        assert!(
            args.contains("--no-tls-verify"),
            "no certificate yet: {args}"
        );
        assert!(!spec.restart_on_crash);

        Ports::FALLBACK.save(&home).unwrap();
        let plain = super::spec(&home, Path::new("/x/cloudflared"), &site(false));
        assert!(plain.args.join(" ").contains("--url http://127.0.0.1:8080"));
    }

    #[test]
    fn has_a_download_for_the_usual_systems() {
        for (os, arch) in [
            (Os::Windows, Arch::X64),
            (Os::Linux, Arch::X64),
            (Os::Linux, Arch::Arm64),
            (Os::Macos, Arch::X64),
            (Os::Macos, Arch::Arm64),
        ] {
            assert!(asset(Platform { os, arch }).is_some());
        }
        assert_eq!(
            asset(Platform {
                os: Os::Windows,
                arch: Arch::Arm64
            }),
            None
        );
    }

    /// Downloads cloudflared for real and asks for a quick tunnel to a tiny
    /// local web server. The tunnel needs trycloudflare.com to answer, so
    /// only the download has to work; the rest is reported.
    #[test]
    #[ignore]
    fn live_share_link() {
        let home = crate::testutil::tempdir();
        let version = install(&home, &mut |_| {}).unwrap();
        println!("cloudflared {version}");
        assert!(program(&home).is_some());
        assert_eq!(super::version(&home).as_deref(), Some(version.as_str()));

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                let body = if request.contains("host: blog.stack") {
                    "hello from blog.stack"
                } else {
                    "wrong host"
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Ports {
            http: port,
            https: port,
        }
        .save(&home)
        .unwrap();
        let supervisor = Supervisor::new(home.join("logs"));
        match start(&home, &supervisor, &site(false)) {
            Ok(link) => {
                let url = link.url.unwrap();
                println!("public link: {url}");
                let mut answer = None;
                for _ in 0..20 {
                    if let Ok(text) = Http::new().get_text(&url) {
                        answer = Some(text);
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
                println!("answer through the link: {answer:?}");
                if let Some(text) = answer {
                    assert_eq!(text, "hello from blog.stack");
                }
                assert_eq!(links(&supervisor, &[site(false)]).len(), 1);
                stop(&supervisor, &site(false)).unwrap();
            }
            Err(e) => println!("no quick tunnel from here: {e}"),
        }
    }
}
