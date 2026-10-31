//! The Stackyard Connect plugin as a way to a site's files, for sites whose
//! FTP or SSH login isn't known.
//!
//! The connection's secret is the plugin's connection code (see
//! [`crate::wordpress::connect`]). Each request goes to the site's
//! `admin-ajax.php` over HTTPS with the code's key in a header; the plugin
//! lists, reads, writes and deletes files in WordPress's folder only, and
//! writes only when the code was made to allow changes. Uploads go in
//! pieces small enough for any host's request limit, into a side file that
//! takes the file's place once complete. There is no shell, so the
//! database goes through Stackyard's helper, as with FTP.

use std::io::{Read, Write};
use std::path::Path;

use serde::Deserialize;

use super::check::{self, Stage, Stopped, Whose};
use super::{io_error, Connection, Entry, OnBytes, Remote};
use crate::wordpress::connect::Link;
use crate::{Error, Result};

/// Bytes sent in one upload request: under the 8 MB PHP allows by default,
/// with room to spare on hosts that set it lower.
const PIECE: usize = 2 * 1024 * 1024;

fn fail(message: impl Into<String>) -> Error {
    Error::Remote(message.into())
}

pub(crate) struct Plugin {
    link: Link,
    agent: ureq::Agent,
    /// WordPress's folder, asked once: a reused session is checked with
    /// `Remote::start_dir` before each use, which over FTP is one quick
    /// command but here would be a web request each time. A code that
    /// stopped working shows on the next real request instead.
    root: Option<String>,
}

#[derive(Deserialize)]
struct Root {
    root: String,
}

#[derive(Deserialize)]
struct Listed {
    name: String,
    dir: bool,
    size: u64,
    modified: i64,
}

#[derive(Deserialize)]
struct Stat {
    size: u64,
    modified: i64,
}

impl Plugin {
    pub(crate) fn connect(connection: &Connection, secret: Option<&str>) -> Result<Plugin> {
        let code = secret.filter(|s| !s.is_empty()).ok_or_else(|| {
            fail(format!(
                "the connection code for {} is missing; paste it again in the connection's settings",
                connection.host
            ))
        })?;
        let mut plugin = Plugin {
            link: Link::parse(code)?,
            agent: crate::wordpress::live::agent(),
            root: None,
        };
        plugin.start_dir()?;
        Ok(plugin)
    }

    fn ask<T: serde::de::DeserializeOwned>(&self, op: &str, path: &str) -> Result<T> {
        self.link.request(&self.agent, op, &[("path", path)])
    }
}

impl Remote for Plugin {
    fn list(&mut self, dir: &str) -> Result<Vec<Entry>> {
        let listed: Vec<Listed> = self.ask("ls", dir)?;
        Ok(listed
            .into_iter()
            .map(|l| Entry {
                name: l.name,
                dir: l.dir,
                size: l.size,
                modified: (l.modified > 0).then_some(l.modified),
            })
            .collect())
    }

    fn download(&mut self, remote: &str, local: &Path, on_bytes: OnBytes) -> Result<()> {
        let stat: Stat = self.ask("stat", remote)?;
        let modified = (stat.modified > 0).then_some(stat.modified);
        let part = super::partial_name(local);
        let (mut file, mut done) = super::open_partial(&part, Some(stat.size), modified)?;
        let mut response = self
            .link
            .get(&self.agent, "get", &[("path", remote)], done)?;
        if response.status().as_u16() == 200 && done > 0 {
            // The server sent all of it; start the file over.
            (file, done) = super::open_partial(&part, None, None)?;
        }
        let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
        let mut buf = vec![0u8; 256 * 1024];
        let copied = loop {
            match reader.read(&mut buf) {
                Ok(0) => break Ok(()),
                Ok(n) => {
                    if let Err(e) = file.write_all(&buf[..n]) {
                        break Err(Error::Io {
                            path: part.clone(),
                            source: e,
                        });
                    }
                    done += n as u64;
                    if !on_bytes(done) {
                        break Err(fail("stopped"));
                    }
                }
                Err(e) => break Err(fail(format!("download failed: {e}"))),
            }
        };
        let copied = copied.and_then(|()| {
            if done == stat.size {
                Ok(())
            } else {
                Err(fail(format!(
                    "the download of {remote} broke off at {done} of {} bytes",
                    stat.size
                )))
            }
        });
        if let Err(e) = copied {
            super::keep_partial(file, &part, modified);
            return Err(e);
        }
        if let Some(at) = modified {
            super::set_mtime(&file, at);
        }
        drop(file);
        std::fs::rename(&part, local).map_err(io_error(local))
    }

    fn upload(&mut self, local: &Path, remote: &str, on_bytes: OnBytes) -> Result<()> {
        let mut file = std::fs::File::open(local).map_err(io_error(local))?;
        let meta = file.metadata().map_err(io_error(local))?;
        let size = meta.len();
        let mtime = super::mtime_of(&meta).unwrap_or(0).to_string();
        let mut buf = vec![0u8; PIECE];
        let mut offset = 0u64;
        loop {
            let mut filled = 0;
            while filled < PIECE {
                let n = file.read(&mut buf[filled..]).map_err(io_error(local))?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            let last = offset + filled as u64 >= size;
            let offset_text = offset.to_string();
            let mut params = vec![("path", remote), ("offset", offset_text.as_str())];
            if last {
                params.push(("last", "1"));
                params.push(("mtime", mtime.as_str()));
            }
            self.link
                .put::<serde_json::Value>(&self.agent, &params, &buf[..filled])?;
            offset += filled as u64;
            if !on_bytes(offset) {
                return Err(fail("stopped"));
            }
            if last {
                return Ok(());
            }
        }
    }

    fn make_dir(&mut self, dir: &str) -> Result<()> {
        self.ask::<serde_json::Value>("mkdir", dir).map(|_| ())
    }

    fn remove_file(&mut self, path: &str) -> Result<()> {
        self.ask::<serde_json::Value>("rm", path).map(|_| ())
    }

    fn start_dir(&mut self) -> Result<String> {
        if let Some(root) = &self.root {
            return Ok(root.clone());
        }
        let root: Root = self.link.request(&self.agent, "root", &[])?;
        self.root = Some(root.root.clone());
        Ok(root.root)
    }
}

/// Checks a plugin connection step by step: the code, then WordPress's
/// folder.
pub(crate) fn check(connection: &Connection, secret: Option<&str>) -> check::Report {
    let mut done = Vec::new();
    let mut plugin = match Plugin::connect(connection, secret) {
        Ok(plugin) => plugin,
        Err(e) => {
            let stop = Stopped::new(
                Stage::Login,
                Whose::Access,
                format!("{} did not take the connection code: {e}.", connection.host),
                "Make a new connection code under Tools › Stackyard Connect on the live site and paste it into this connection. A code stops working after a day without use.",
            );
            return check::report(connection.protocol, done, Some(stop), None);
        }
    };
    done.push(check::ok(
        Stage::Login,
        format!("{} took the connection code.", connection.host),
    ));
    let listed = plugin
        .start_dir()
        .and_then(|root| plugin.list(&root).map(|l| (root, l.len())));
    let stop = match listed {
        Ok((root, count)) => {
            done.push(check::ok(
                Stage::Files,
                format!("WordPress's folder {root} holds {count} files and folders."),
            ));
            None
        }
        Err(e) => Some(Stopped::new(
            Stage::Files,
            Whose::Server,
            format!("Listing WordPress's folder failed: {e}."),
            "Update the Stackyard Connect plugin on the live site.",
        )),
    };
    check::report(connection.protocol, done, stop, None)
}
