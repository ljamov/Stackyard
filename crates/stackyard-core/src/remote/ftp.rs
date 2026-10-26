//! A small FTP client (RFC 959), with explicit TLS (RFC 4217) for FTPS,
//! passive mode only, and `MLSD` listings (RFC 3659) where the server has
//! them, else Unix- or Windows-style `LIST` output.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
use sha2::{Digest, Sha256};

use super::check::{self, Stage, Stopped, Whose};
use super::{io_error, partial_name, set_mtime, Connection, Entry, OnBytes, Protocol, Remote};
use crate::{Error, Result};

const TIMEOUT: Duration = Duration::from_secs(30);
const CHUNK: usize = 64 * 1024;

fn fail(message: impl Into<String>) -> Error {
    Error::Remote(message.into())
}

enum Stream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
    Closed,
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
            Stream::Closed => Err(std::io::ErrorKind::NotConnected.into()),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
            Stream::Closed => Err(std::io::ErrorKind::NotConnected.into()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
            Stream::Closed => Ok(()),
        }
    }
}

impl Stream {
    /// Ends the stream cleanly, so the server knows a TLS upload is complete.
    fn finish(self) {
        match self {
            Stream::Tls(mut s) => {
                s.conn.send_close_notify();
                let _ = s.flush();
                let _ = s.sock.shutdown(std::net::Shutdown::Write);
            }
            Stream::Plain(s) => {
                let _ = s.shutdown(std::net::Shutdown::Write);
            }
            Stream::Closed => {}
        }
    }
}

/// Accepts what the system trusts, or the one certificate the user chose to
/// trust; remembers the certificate seen so an error can name it.
#[derive(Debug)]
struct Verifier {
    system: Arc<dyn ServerCertVerifier>,
    pinned: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
}

pub(crate) fn fingerprint(der: &[u8]) -> String {
    let hash = Sha256::digest(der);
    let hex: Vec<String> = hash.iter().map(|b| format!("{b:02X}")).collect();
    format!("SHA256:{}", hex.join(":"))
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let seen = fingerprint(end_entity.as_ref());
        *self.seen.lock().unwrap() = Some(seen.clone());
        if self.pinned.as_deref() == Some(seen.as_str()) {
            return Ok(ServerCertVerified::assertion());
        }
        self.system
            .verify_server_cert(end_entity, intermediates, server_name, ocsp, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.system.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.system.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.system.supported_verify_schemes()
    }
}

struct Tls {
    config: Arc<ClientConfig>,
    name: ServerName<'static>,
    seen: Arc<Mutex<Option<String>>>,
}

impl Tls {
    fn new(host: &str, pinned: Option<String>) -> Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let system = rustls_platform_verifier::Verifier::new(provider.clone())
            .map_err(|e| fail(format!("could not load the system's certificates: {e}")))?;
        let seen = Arc::new(Mutex::new(None));
        let verifier = Verifier {
            system: Arc::new(system),
            pinned,
            seen: seen.clone(),
        };
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| fail(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        let name = ServerName::try_from(host.to_string())
            .map_err(|_| fail(format!("{host:?} is not a valid server name")))?;
        Ok(Self {
            config: Arc::new(config),
            name,
            seen,
        })
    }

    /// Wraps a connection in TLS and completes the handshake, so a refused
    /// certificate shows up here with a useful message.
    fn wrap(&self, mut tcp: TcpStream) -> Result<Stream> {
        let mut conn = ClientConnection::new(self.config.clone(), self.name.clone())
            .map_err(|e| fail(format!("TLS failed: {e}")))?;
        while conn.is_handshaking() {
            if let Err(e) = conn.complete_io(&mut tcp) {
                return Err(self.explain(e));
            }
        }
        if conn.protocol_version() == Some(rustls::ProtocolVersion::TLSv1_3) {
            take_tickets(&mut conn, &mut tcp);
        }
        Ok(Stream::Tls(Box::new(StreamOwned::new(conn, tcp))))
    }

    fn explain(&self, e: std::io::Error) -> Error {
        let certificate = e
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<rustls::Error>())
            .is_some_and(|tls| matches!(tls, rustls::Error::InvalidCertificate(_)));
        match self.seen.lock().unwrap().clone() {
            Some(seen) if certificate => Error::RemoteCertificate {
                fingerprint: seen,
                reason: e.to_string(),
            },
            _ => fail(format!("TLS failed: {e}")),
        }
    }
}

/// Waits briefly for the session tickets a TLS 1.3 server sends right after
/// the handshake. Many FTPS servers only accept data connections that
/// resume the control connection's session, and each ticket can be used
/// once; an upload never reads from its connection, so without this the
/// tickets would run out after a few files.
fn take_tickets(conn: &mut ClientConnection, tcp: &mut TcpStream) {
    let before = tcp.read_timeout().ok().flatten();
    let _ = tcp.set_read_timeout(Some(Duration::from_millis(500)));
    if let Ok(n) = conn.read_tls(tcp) {
        if n > 0 {
            let _ = conn.process_new_packets();
        }
    }
    let _ = tcp.set_read_timeout(before);
}

/// The control connection: commands go out, numbered replies come back.
struct Control {
    stream: Stream,
    pending: Vec<u8>,
}

impl Control {
    fn read_line(&mut self) -> Result<String> {
        loop {
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=end).collect();
                return Ok(String::from_utf8_lossy(&line)
                    .trim_end_matches(['\r', '\n'])
                    .to_string());
            }
            let mut buf = [0u8; 1024];
            let n = self
                .stream
                .read(&mut buf)
                .map_err(|e| fail(format!("the server stopped answering: {e}")))?;
            if n == 0 {
                return Err(fail("the server closed the connection"));
            }
            self.pending.extend_from_slice(&buf[..n]);
        }
    }

    /// A reply: its code and its text, all lines of a multi-line reply joined.
    fn reply(&mut self) -> Result<(u16, String)> {
        let first = self.read_line()?;
        let code: u16 = first
            .get(..3)
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| fail(format!("not an FTP server (it said {first:?})")))?;
        let mut text = first.get(4..).unwrap_or("").to_string();
        if first.as_bytes().get(3) == Some(&b'-') {
            let end = format!("{code} ");
            loop {
                let line = self.read_line()?;
                let last = line.starts_with(&end) || line == code.to_string();
                text.push('\n');
                text.push_str(line.strip_prefix(&end).unwrap_or(&line));
                if last {
                    break;
                }
            }
        }
        Ok((code, text))
    }

    fn send(&mut self, command: &str) -> Result<(u16, String)> {
        self.stream
            .write_all(format!("{command}\r\n").as_bytes())
            .and_then(|_| self.stream.flush())
            .map_err(|e| fail(format!("the server stopped answering: {e}")))?;
        self.reply()
    }

    /// Sends a command and fails with the server's words unless the reply
    /// code is in `ok`. `shown` is the command as errors name it.
    fn expect(&mut self, command: &str, shown: &str, ok: &[u16]) -> Result<String> {
        let (code, text) = self.send(command)?;
        if ok.contains(&code) {
            Ok(text)
        } else {
            Err(fail(format!(
                "{shown}: the server said {code} {}",
                text.trim()
            )))
        }
    }
}

pub(crate) struct Ftp {
    control: Control,
    tls: Option<Tls>,
    peer: IpAddr,
    mlsd: bool,
    mfmt: bool,
}

fn connect_tcp(host: &str, port: u16) -> Result<TcpStream> {
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| fail(format!("could not find {host}: {e}")))?
        .collect();
    let mut last = None;
    for addr in &addrs {
        match TcpStream::connect_timeout(addr, TIMEOUT) {
            Ok(tcp) => {
                let _ = tcp.set_read_timeout(Some(TIMEOUT));
                let _ = tcp.set_write_timeout(Some(TIMEOUT));
                let _ = tcp.set_nodelay(true);
                return Ok(tcp);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(fail(match last {
        Some(e) => format!("could not connect to {host} port {port}: {e}"),
        None => format!("could not find {host}"),
    }))
}

impl Ftp {
    pub(crate) fn connect(connection: &Connection, password: Option<&str>) -> Result<Self> {
        let addrs = check::find(&connection.host, connection.port).map_err(Stopped::into_error)?;
        let tcp = check::reach(connection, &addrs).map_err(Stopped::into_error)?;
        let _ = tcp.set_read_timeout(Some(TIMEOUT));
        let _ = tcp.set_write_timeout(Some(TIMEOUT));
        let _ = tcp.set_nodelay(true);
        let peer = tcp.peer_addr().map_err(|e| fail(e.to_string()))?.ip();
        let mut control = Control {
            stream: Stream::Plain(tcp),
            pending: Vec::new(),
        };
        let (code, text) = control.reply()?;
        if code != 220 {
            return Err(fail(format!(
                "the server refused the connection: {code} {text}"
            )));
        }
        let tls = if connection.protocol == Protocol::Ftps {
            control
                .expect("AUTH TLS", "Switching to TLS", &[234])
                .map_err(|_| fail("this server does not offer FTPS; choose FTP or SFTP instead"))?;
            let tls = Tls::new(&connection.host, connection.server_key.clone())?;
            let Stream::Plain(tcp) = std::mem::replace(&mut control.stream, Stream::Closed) else {
                unreachable!("TLS starts once, on a plain connection")
            };
            control.stream = tls.wrap(tcp)?;
            Some(tls)
        } else {
            None
        };

        let (code, text) = control.send(&format!("USER {}", connection.user))?;
        match code {
            230 => {}
            331 | 332 => {
                let password = password.unwrap_or("");
                let (code, text) = control.send(&format!("PASS {password}"))?;
                if code != 230 && code != 202 {
                    return Err(fail(format!(
                        "the server refused the user name or password ({code} {})",
                        text.trim()
                    )));
                }
            }
            _ => {
                return Err(fail(format!(
                    "the server refused the user name ({code} {})",
                    text.trim()
                )))
            }
        }
        if tls.is_some() {
            control.expect("PBSZ 0", "Setting up TLS", &[200])?;
            control.expect("PROT P", "Encrypting transfers", &[200])?;
        }
        let features = match control.send("FEAT")? {
            (211, text) => text.to_ascii_uppercase(),
            _ => String::new(),
        };
        if features.contains("UTF8") {
            let _ = control.send("OPTS UTF8 ON");
        }
        control.expect("TYPE I", "Switching to binary transfers", &[200])?;
        Ok(Self {
            control,
            tls,
            peer,
            mlsd: features.contains("MLSD") || features.contains("MLST"),
            mfmt: features.contains("MFMT"),
        })
    }

    /// Opens a passive data connection and starts `command` on it.
    fn data(&mut self, command: &str) -> Result<Stream> {
        let port = match self.control.send("EPSV")? {
            (229, text) => parse_epsv(&text),
            _ => None,
        };
        let port = match port {
            Some(port) => port,
            None => {
                let (code, text) = self.control.send("PASV")?;
                if code != 227 {
                    return Err(fail(format!(
                        "the server refused passive mode: {code} {text}"
                    )));
                }
                // The address in the reply is often a private one behind NAT;
                // the server we are talking to is the one to connect to.
                parse_pasv(&text)
                    .ok_or_else(|| fail(format!("could not read the server's reply {text:?}")))?
            }
        };
        let tcp = connect_tcp(&self.peer.to_string(), port)?;
        let (code, text) = self.control.send(command)?;
        if !(100..200).contains(&code) {
            return Err(fail(format!(
                "{}: the server said {code} {}",
                command_name(command),
                text.trim()
            )));
        }
        match &self.tls {
            Some(tls) => tls.wrap(tcp),
            None => Ok(Stream::Plain(tcp)),
        }
    }

    /// Reads the reply that ends a transfer.
    fn done(&mut self, command: &str) -> Result<()> {
        let (code, text) = self.control.reply()?;
        if (200..300).contains(&code) {
            Ok(())
        } else {
            Err(fail(format!(
                "{}: the server said {code} {}",
                command_name(command),
                text.trim()
            )))
        }
    }

    fn read_all(&mut self, command: &str) -> Result<String> {
        let mut stream = self.data(command)?;
        let mut bytes = Vec::new();
        stream
            .read_to_end(&mut bytes)
            .map_err(|e| fail(format!("listing failed: {e}")))?;
        stream.finish();
        self.done(command)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn modified(&mut self, path: &str) -> Option<i64> {
        match self.control.send(&format!("MDTM {path}")) {
            Ok((213, text)) => parse_time(text.trim()),
            _ => None,
        }
    }
}

fn command_name(command: &str) -> &str {
    match command.split_once(' ').map_or(command, |(c, _)| c) {
        "RETR" => "Download",
        "STOR" => "Upload",
        "MLSD" | "LIST" => "Listing",
        other => other,
    }
}

impl Drop for Ftp {
    fn drop(&mut self) {
        let _ = self.control.stream.write_all(b"QUIT\r\n");
    }
}

impl Remote for Ftp {
    fn list(&mut self, dir: &str) -> Result<Vec<Entry>> {
        if self.mlsd {
            let text = self.read_all(&format!("MLSD {dir}"))?;
            return Ok(text.lines().filter_map(parse_mlsd).collect());
        }
        let text = self
            .read_all(&format!("LIST -a {dir}"))
            .or_else(|_| self.read_all(&format!("LIST {dir}")))?;
        Ok(text.lines().filter_map(parse_list).collect())
    }

    fn download(&mut self, remote: &str, local: &Path, on_bytes: OnBytes) -> Result<()> {
        let modified = self.modified(remote);
        let size = match self.control.send(&format!("SIZE {remote}")) {
            Ok((213, text)) => text.trim().parse().ok(),
            _ => None,
        };
        let part = partial_name(local);
        let (mut file, mut done) = super::open_partial(&part, size, modified)?;
        if done > 0 && !matches!(self.control.send(&format!("REST {done}")), Ok((350, _))) {
            (file, done) = super::open_partial(&part, None, None)?;
        }
        let command = format!("RETR {remote}");
        let mut stream = match self.data(&command) {
            Ok(stream) => stream,
            Err(e) => {
                super::keep_partial(file, &part, modified);
                return Err(e);
            }
        };
        let mut buf = vec![0u8; CHUNK];
        let copied = loop {
            match stream.read(&mut buf) {
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
        stream.finish();
        let finished = self.done(&command);
        if let Err(e) = copied.and(finished) {
            super::keep_partial(file, &part, modified);
            return Err(e);
        }
        if let Some(at) = modified {
            set_mtime(&file, at);
        }
        drop(file);
        std::fs::rename(&part, local).map_err(io_error(local))
    }

    fn upload(&mut self, local: &Path, remote: &str, on_bytes: OnBytes) -> Result<()> {
        let mut file = std::fs::File::open(local).map_err(io_error(local))?;
        let modified = file.metadata().ok().as_ref().and_then(super::mtime_of);
        let command = format!("STOR {remote}");
        let mut stream = self.data(&command)?;
        let mut buf = vec![0u8; CHUNK];
        let mut done = 0u64;
        let copied = loop {
            match file.read(&mut buf) {
                Ok(0) => break Ok(()),
                Ok(n) => {
                    if let Err(e) = stream.write_all(&buf[..n]) {
                        break Err(fail(format!("upload failed: {e}")));
                    }
                    done += n as u64;
                    if !on_bytes(done) {
                        break Err(fail("stopped"));
                    }
                }
                Err(e) => {
                    break Err(Error::Io {
                        path: local.to_owned(),
                        source: e,
                    })
                }
            }
        };
        stream.finish();
        let finished = self.done(&command);
        copied.and(finished)?;
        if let (true, Some(at)) = (self.mfmt, modified) {
            let _ = self
                .control
                .send(&format!("MFMT {} {remote}", format_time(at)));
        }
        Ok(())
    }

    fn make_dir(&mut self, dir: &str) -> Result<()> {
        match self.control.send(&format!("MKD {dir}"))? {
            (257 | 250, _) => Ok(()),
            // Most likely there already; check by going into it.
            (code, text) => match self.control.send(&format!("CWD {dir}"))? {
                (250, _) => Ok(()),
                _ => Err(fail(format!(
                    "could not make {dir}: {code} {}",
                    text.trim()
                ))),
            },
        }
    }

    fn remove_file(&mut self, path: &str) -> Result<()> {
        self.control
            .expect(&format!("DELE {path}"), &format!("Deleting {path}"), &[250])
            .map(|_| ())
    }

    fn start_dir(&mut self) -> Result<String> {
        let text = self
            .control
            .expect("PWD", "Finding the start folder", &[257])?;
        // 257 "/home/user" is the current directory ("" inside is doubled).
        let start = text.find('"').map(|i| i + 1).unwrap_or(0);
        let mut path = String::new();
        let mut chars = text[start..].chars().peekable();
        while let Some(c) = chars.next() {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                } else {
                    break;
                }
            }
            path.push(c);
        }
        Ok(if path.is_empty() { "/".into() } else { path })
    }
}

/// `Entering Extended Passive Mode (|||6446|)` → 6446.
fn parse_epsv(text: &str) -> Option<u16> {
    let inside = text.split_once('(')?.1.split_once(')')?.0;
    let delimiter = inside.chars().next()?;
    inside.split(delimiter).nth(3)?.parse().ok()
}

/// `Entering Passive Mode (192,168,1,2,19,137)` → 19 * 256 + 137.
fn parse_pasv(text: &str) -> Option<u16> {
    let start = text.find(|c: char| c.is_ascii_digit() && text.contains(','))?;
    let numbers: Vec<u16> = text[start..]
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .take(6)
        .map(|s| s.parse().ok())
        .collect::<Option<_>>()?;
    (numbers.len() == 6 && numbers[4] < 256 && numbers[5] < 256)
        .then(|| numbers[4] * 256 + numbers[5])
}

/// `type=file;size=1234;modify=20240131120000; index.php`
fn parse_mlsd(line: &str) -> Option<Entry> {
    let (facts, name) = line.split_once(' ')?;
    let mut entry = Entry {
        name: name.to_string(),
        dir: false,
        size: 0,
        modified: None,
    };
    for fact in facts.split(';') {
        let Some((key, value)) = fact.split_once('=') else {
            continue;
        };
        match key.to_ascii_lowercase().as_str() {
            "type" => match value.to_ascii_lowercase().as_str() {
                "dir" => entry.dir = true,
                "cdir" | "pdir" => return None,
                v if v.starts_with("os.unix=slink") => return None,
                _ => {}
            },
            "size" => entry.size = value.parse().unwrap_or(0),
            "modify" => entry.modified = parse_time(value),
            _ => {}
        }
    }
    (!entry.name.is_empty()).then_some(entry)
}

/// A `LIST` line, either Unix style
/// (`-rw-r--r-- 1 owner group 1234 Jan 31 12:00 index.php`) or Windows
/// style (`01-31-24  12:00PM  <DIR>  assets`). Times there are too rough
/// to compare, so they are left out.
fn parse_list(line: &str) -> Option<Entry> {
    let mut rest = line.trim_end();
    let mut fields = Vec::new();
    let count = if rest.chars().next()?.is_ascii_digit() {
        3
    } else {
        8
    };
    for _ in 0..count {
        rest = rest.trim_start();
        let end = rest.find(char::is_whitespace)?;
        fields.push(&rest[..end]);
        rest = &rest[end..];
    }
    let name = rest.trim_start().to_string();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    if count == 3 {
        let dir = fields[2].eq_ignore_ascii_case("<DIR>");
        return Some(Entry {
            name,
            dir,
            size: if dir { 0 } else { fields[2].parse().ok()? },
            modified: None,
        });
    }
    match fields[0].chars().next()? {
        'd' => Some(Entry {
            name,
            dir: true,
            size: 0,
            modified: None,
        }),
        '-' => Some(Entry {
            name,
            dir: false,
            size: fields[4].parse().ok()?,
            modified: None,
        }),
        // Links and devices are left alone.
        _ => None,
    }
}

/// `20240131120000` or `20240131120000.123` (UTC) → seconds since 1970.
fn parse_time(value: &str) -> Option<i64> {
    let digits = value.split('.').next()?;
    if digits.len() != 14 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = |range: std::ops::Range<usize>| digits[range].parse::<i64>().ok();
    let (year, month, day) = (n(0..4)?, n(4..6)?, n(6..8)?);
    let (hour, minute, second) = (n(8..10)?, n(10..12)?, n(12..14)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second)
}

/// Seconds since 1970 → `20240131120000` (UTC).
fn format_time(at: i64) -> String {
    let m = crate::scheduler::clock::utc(at);
    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}",
        m.year,
        m.month,
        m.day,
        m.hour,
        m.minute,
        at.rem_euclid(60)
    )
}

/// Days since 1970-01-01 for a date in the proleptic Gregorian calendar,
/// counting years from March so the leap day falls last.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Finds and reaches the server, logs in, and lists the start folder.
pub(crate) fn check(connection: &Connection, password: Option<&str>) -> check::Report {
    let mut done = Vec::new();
    let reached = check::find(&connection.host, connection.port).and_then(|addrs| {
        done.push(check::ok(
            Stage::Find,
            format!("{} is at {}.", connection.host, addrs[0].ip()),
        ));
        check::reach(connection, &addrs)
    });
    if let Err(stop) = reached {
        return check::report(connection.protocol, done, Some(stop), None);
    }
    done.push(check::ok(
        Stage::Reach,
        format!("Port {} answered.", connection.port),
    ));
    let mut ftp = match Ftp::connect(connection, password) {
        Ok(ftp) => ftp,
        Err(e) => {
            let text = e.to_string();
            let mut stop = if text.contains("refused the user name") {
                check::login_refused(
                    connection,
                    false,
                    Some(text.trim_start_matches("the server ").to_string()),
                )
            } else if matches!(e, Error::RemoteCertificate { .. }) {
                Stopped::new(
                    Stage::Login,
                    Whose::Server,
                    format!("The server's certificate is not trusted ({text})."),
                    "If this is your host's server, trust the certificate below. Hosts with shared servers often use a certificate made out to the server's own name instead of your domain.",
                )
            } else if text.contains("does not offer FTPS") {
                Stopped::new(
                    Stage::Login,
                    Whose::Server,
                    "This server does not offer FTPS.",
                    "Connect with SFTP (best) or plain FTP instead.",
                )
            } else {
                Stopped::new(
                    Stage::Login,
                    Whose::Server,
                    format!("Logging in failed: {text}."),
                    "Check the user name, password and port, then try again in a few minutes.",
                )
            };
            if matches!(e, Error::RemoteCertificate { .. }) {
                stop.typed = Some(Box::new(e));
            }
            return check::report(connection.protocol, done, Some(stop), None);
        }
    };
    done.push(check::ok(
        Stage::Login,
        format!("Logged in as {}.", connection.user),
    ));
    let stop = match ftp.start_dir() {
        Ok(dir) => {
            done.push(check::ok(
                Stage::Files,
                format!("Files can be listed; the server starts in {dir}."),
            ));
            None
        }
        Err(e) => Some(Stopped::new(
            Stage::Files,
            Whose::Internet,
            format!("Logged in, but listing files failed: {e}."),
            "FTP opens a second connection for each listing; a firewall on this computer or the network may block it. Try SFTP, or allow Stackyard in the firewall.",
        )),
    };
    check::report(connection.protocol, done, stop, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_passive_replies() {
        assert_eq!(
            parse_epsv("Entering Extended Passive Mode (|||6446|)"),
            Some(6446)
        );
        assert_eq!(
            parse_epsv("Entering Extended Passive Mode (!!!21000!)"),
            Some(21000)
        );
        assert_eq!(
            parse_pasv("Entering Passive Mode (192,168,1,2,19,137)."),
            Some(19 * 256 + 137)
        );
        assert_eq!(parse_pasv("Entering Passive Mode 10,0,0,1,4,1"), Some(1025));
        assert_eq!(parse_pasv("Entering Passive Mode"), None);
    }

    #[test]
    fn reads_listings() {
        let file =
            parse_mlsd("type=file;size=1234;modify=20240131120000.5;perm=r; my file.php").unwrap();
        assert_eq!(file.name, "my file.php");
        assert_eq!(file.size, 1234);
        assert!(!file.dir);
        assert_eq!(file.modified, Some(1_706_702_400));
        assert!(
            parse_mlsd("Type=dir;Modify=20240131120000; assets")
                .unwrap()
                .dir
        );
        assert!(parse_mlsd("type=cdir; .").is_none());

        let unix =
            parse_list("-rw-r--r--    1 owner    group        1234 Jan 31 12:00 index copy.php")
                .unwrap();
        assert_eq!(
            (unix.name.as_str(), unix.size, unix.dir),
            ("index copy.php", 1234, false)
        );
        assert!(
            parse_list("drwxr-xr-x    2 owner    group        4096 Jan 31  2023 assets")
                .unwrap()
                .dir
        );
        assert!(
            parse_list("lrwxrwxrwx    1 owner    group          10 Jan 31 12:00 l -> x").is_none()
        );
        let dos = parse_list("01-31-24  12:00PM       <DIR>          wp-content").unwrap();
        assert!(dos.dir && dos.name == "wp-content");
        let dos_file = parse_list("01-31-24  12:00PM                 5120 web.config").unwrap();
        assert_eq!(dos_file.size, 5120);
    }

    #[test]
    fn converts_times_both_ways() {
        assert_eq!(parse_time("19700101000000"), Some(0));
        assert_eq!(parse_time("20000229235959"), Some(951_868_799));
        assert_eq!(parse_time("2024013112"), None);
        assert_eq!(format_time(951_868_799), "20000229235959");
        assert_eq!(parse_time(&format_time(1_760_000_123)), Some(1_760_000_123));
    }

    #[test]
    fn fingerprints_certificates() {
        let fp = fingerprint(b"abc");
        assert!(fp.starts_with("SHA256:BA:78:16:BF"));
        assert_eq!(fp.len(), "SHA256:".len() + 32 * 3 - 1);
    }

    /// A tiny FTP server for one session, enough to drive the client.
    fn serve(
        script: impl FnOnce(&mut std::io::BufReader<TcpStream>, &mut TcpStream) + Send + 'static,
    ) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut out, _) = listener.accept().unwrap();
            let mut input = std::io::BufReader::new(out.try_clone().unwrap());
            script(&mut input, &mut out);
        });
        port
    }

    fn expect_line(input: &mut std::io::BufReader<TcpStream>, start: &str) -> String {
        use std::io::BufRead;
        let mut line = String::new();
        input.read_line(&mut line).unwrap();
        assert!(line.starts_with(start), "expected {start}, got {line:?}");
        line.trim_end().to_string()
    }

    #[test]
    fn logs_in_lists_and_downloads_over_plain_ftp() {
        let port = serve(|input, out| {
            let data = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let data_port = data.local_addr().unwrap().port();
            let mut say = |text: &str| out.write_all(format!("{text}\r\n").as_bytes()).unwrap();
            say("220-Welcome\r\n220 ready");
            expect_line(input, "USER sam");
            say("331 password please");
            expect_line(input, "PASS secret");
            say("230 in");
            expect_line(input, "FEAT");
            say("211-Features:\r\n MLSD\r\n MFMT\r\n211 End");
            expect_line(input, "TYPE I");
            say("200 ok");
            expect_line(input, "PWD");
            say("257 \"/home/sam\" is the current directory");
            for (command, body) in [
                ("MLSD /home/sam", "type=dir;modify=20240131120000; www\r\ntype=file;size=5;modify=20240131120000; a.txt\r\n"),
                ("RETR /home/sam/a.txt", "llo"),
            ] {
                if command.starts_with("RETR") {
                    expect_line(input, "MDTM /home/sam/a.txt");
                    say("213 20240131120000");
                    expect_line(input, "SIZE /home/sam/a.txt");
                    say("213 5");
                    // The first two bytes came in an earlier, broken-off try.
                    expect_line(input, "REST 2");
                    say("350 restarting at 2");
                }
                expect_line(input, "EPSV");
                say(&format!("229 Entering Extended Passive Mode (|||{data_port}|)"));
                let (mut channel, _) = data.accept().unwrap();
                expect_line(input, command);
                say("150 here it comes");
                channel.write_all(body.as_bytes()).unwrap();
                drop(channel);
                say("226 done");
            }
            expect_line(input, "QUIT");
        });
        let connection = Connection {
            id: "test".into(),
            site: "a.stack".into(),
            name: "Test".into(),
            protocol: Protocol::Ftp,
            host: "127.0.0.1".into(),
            port,
            user: "sam".into(),
            key_file: None,
            remote_dir: String::new(),
            local_dir: String::new(),
            skip: Vec::new(),
            server_key: None,
        };
        let mut ftp = Ftp::connect(&connection, Some("secret")).unwrap();
        assert_eq!(ftp.start_dir().unwrap(), "/home/sam");
        let entries = ftp.list("/home/sam").unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].dir);
        let target = crate::testutil::tempdir().join("a.txt");
        let part = super::super::partial_name(&target);
        std::fs::write(&part, "he").unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&part).unwrap();
        super::super::set_mtime(&file, parse_time("20240131120000").unwrap());
        drop(file);
        let mut seen = 0;
        ftp.download("/home/sam/a.txt", &target, &mut |n| {
            seen = n;
            true
        })
        .unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello");
        assert_eq!(seen, 5);
        let meta = std::fs::metadata(&target).unwrap();
        assert_eq!(super::super::mtime_of(&meta), Some(1_706_702_400));
    }
}
