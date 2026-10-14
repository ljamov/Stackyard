//! HTTPS downloads. Every file Stackyard installs is checked against a
//! SHA-256 checksum published by its source before anything is unpacked.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use ureq::tls::{RootCerts, TlsConfig};

use crate::{Error, Result};

/// Largest index or checksum file we will read into memory.
const TEXT_LIMIT: u64 = 16 * 1024 * 1024;

pub struct Http {
    agent: ureq::Agent,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Http {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            // The OS certificate store, so company proxies and custom CAs work.
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .user_agent(concat!("Stackyard/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Self { agent }
    }

    /// Fetches a small text file such as a release index.
    pub fn get_text(&self, url: &str) -> Result<String> {
        let fail = |e: ureq::Error| download_error(url, e);
        let mut response = self.agent.get(url).call().map_err(fail)?;
        response
            .body_mut()
            .with_config()
            .limit(TEXT_LIMIT)
            .read_to_string()
            .map_err(fail)
    }

    /// Downloads `url` to `dest`, checking it against `sha256` (hex).
    ///
    /// Data goes to `dest` + `.part` first and is only renamed into place once
    /// the checksum matches, so `dest` is either complete and verified or absent.
    /// `progress` receives (bytes so far, total size if known).
    pub fn download(
        &self,
        url: &str,
        dest: &Path,
        sha256: &str,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<()> {
        let fail = |e: ureq::Error| download_error(url, e);
        let io = |source| Error::Io {
            path: dest.to_owned(),
            source,
        };
        let mut response = self.agent.get(url).call().map_err(fail)?;
        let total = response.body().content_length();
        let mut reader = response.body_mut().as_reader();

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let part = with_suffix(dest, ".part");
        let mut file = File::create(&part).map_err(io)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0; 64 * 1024];
        let mut done = 0u64;
        let mut last_report = Instant::now();
        progress(0, total);
        let result = loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break Ok(()),
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    break Err(Error::Download {
                        url: url.to_string(),
                        message: e.to_string(),
                    })
                }
            };
            hasher.update(&buf[..n]);
            if let Err(e) = file.write_all(&buf[..n]) {
                break Err(io(e));
            }
            done += n as u64;
            if last_report.elapsed() >= Duration::from_millis(100) {
                progress(done, total);
                last_report = Instant::now();
            }
        };
        let result = result
            .and_then(|()| file.sync_all().map_err(io))
            .and_then(|()| check_sha256(dest, sha256, &hex(&hasher.finalize())));
        drop(file);
        if let Err(e) = result {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }
        progress(done, total);
        std::fs::rename(&part, dest).map_err(io)
    }
}

/// SHA-256 of a file on disk, as lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    let io = |source| Error::Io {
        path: path.to_owned(),
        source,
    };
    let mut file = File::open(path).map_err(io)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(io)?;
    Ok(hex(&hasher.finalize()))
}

/// Finds a file's checksum in a `sha256sum`-style list
/// (`<hex>  name` or `<hex> *name`, one per line).
pub fn find_checksum(list: &str, file_name: &str) -> Option<String> {
    list.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        (name == file_name && is_sha256(hash)).then(|| hash.to_ascii_lowercase())
    })
}

pub fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn check_sha256(dest: &Path, expected: &str, actual: &str) -> Result<()> {
    if expected.eq_ignore_ascii_case(actual) {
        Ok(())
    } else {
        Err(Error::Checksum {
            file: dest
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            expected: expected.to_ascii_lowercase(),
            actual: actual.to_string(),
        })
    }
}

fn download_error(url: &str, e: ureq::Error) -> Error {
    let message = match e {
        ureq::Error::StatusCode(404) => "file not found (404)".to_string(),
        ureq::Error::StatusCode(code) => format!("server answered {code}"),
        other => other.to_string(),
    };
    Error::Download {
        url: url.to_string(),
        message,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    s.into()
}

#[cfg(test)]
pub(crate) mod testserver {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// Serves `body` to every request on a local port and returns the base URL.
    /// Paths ending in `/missing` get a 404.
    pub fn serve(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                let _ = reader.read_line(&mut request_line);
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let missing = request_line.contains("/missing ");
                let (status, payload) = if missing {
                    ("404 Not Found", &b""[..])
                } else {
                    ("200 OK", &body[..])
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = stream.write_all(payload);
            }
        });
        format!("http://{addr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO_SHA: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    #[test]
    fn finds_checksums_in_both_list_styles() {
        let list = format!(
            "{HELLO_SHA}  node-v22.1.0-linux-x64.tar.gz\n\
             {} *php-8.4.1-nts-Win32-vs17-x64.zip\n",
            HELLO_SHA.to_uppercase()
        );
        assert_eq!(
            find_checksum(&list, "node-v22.1.0-linux-x64.tar.gz").as_deref(),
            Some(HELLO_SHA)
        );
        assert_eq!(
            find_checksum(&list, "php-8.4.1-nts-Win32-vs17-x64.zip").as_deref(),
            Some(HELLO_SHA)
        );
        assert_eq!(find_checksum(&list, "node-v22.1.0-linux-x64"), None);
        assert_eq!(find_checksum("nothex  a.zip", "a.zip"), None);
    }

    #[test]
    fn downloads_and_verifies() {
        let base = testserver::serve(b"hello".to_vec());
        let dir = crate::testutil::tempdir();
        let dest = dir.join("hello.txt");
        let mut seen = Vec::new();
        Http::new()
            .download(
                &format!("{base}/hello.txt"),
                &dest,
                HELLO_SHA,
                &mut |d, t| seen.push((d, t)),
            )
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
        assert_eq!(seen.last(), Some(&(5, Some(5))));
        assert_eq!(sha256_file(&dest).unwrap(), HELLO_SHA);
    }

    #[test]
    fn rejects_a_wrong_checksum_and_leaves_nothing_behind() {
        let base = testserver::serve(b"tampered".to_vec());
        let dir = crate::testutil::tempdir();
        let dest = dir.join("hello.txt");
        let err = Http::new()
            .download(
                &format!("{base}/hello.txt"),
                &dest,
                HELLO_SHA,
                &mut |_, _| {},
            )
            .unwrap_err();
        assert!(matches!(err, Error::Checksum { .. }), "{err}");
        assert!(!dest.exists());
        assert!(!with_suffix(&dest, ".part").exists());
    }

    #[test]
    fn reports_missing_files() {
        let base = testserver::serve(Vec::new());
        let err = Http::new()
            .get_text(&format!("{base}/missing"))
            .unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");
    }
}
