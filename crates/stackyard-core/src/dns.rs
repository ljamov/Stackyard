//! A tiny DNS server that answers every name under Stackyard's domain
//! endings (`myapp.stack`, `api.myapp.stack`) with 127.0.0.1.
//!
//! macOS and Linux send only those endings here (system setup points them
//! at it); every other lookup keeps using the normal DNS servers. Anything
//! else that reaches this server is refused, so it never answers for the
//! wider internet. Windows uses the hosts file instead (see [`crate::system`]).

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::{Error, Result};

/// Where the server listens, on 127.0.0.1 only.
pub const PORT: u16 = 53535;

const TTL: u32 = 60;
const TYPE_A: u16 = 1;
const TYPE_ANY: u16 = 255;
const CLASS_IN: u16 = 1;
const NO_ERROR: u8 = 0;
const FORMAT_ERROR: u8 = 1;
const REFUSED: u8 = 5;

pub struct DnsServer {
    endings: Arc<RwLock<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    port: u16,
}

impl DnsServer {
    /// Starts answering for `endings` (e.g. `.stack`). Port 0 picks a free one.
    pub fn start(port: u16, endings: &[String]) -> Result<Self> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let socket = UdpSocket::bind(addr).map_err(|_| Error::PortInUse {
            name: "Local domains".into(),
            port,
        })?;
        let port = socket.local_addr().map(|a| a.port()).unwrap_or(port);
        // Wake up regularly to notice `stop`.
        let _ = socket.set_read_timeout(Some(Duration::from_millis(250)));
        let endings = Arc::new(RwLock::new(normalize(endings)));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (endings, stop) = (endings.clone(), stop.clone());
            std::thread::Builder::new()
                .name("stackyard-dns".into())
                .spawn(move || serve(&socket, &endings, &stop))
                .map_err(|source| Error::Spawn {
                    program: "DNS server".into(),
                    source,
                })?
        };
        Ok(Self {
            endings,
            stop,
            thread: Some(thread),
            port,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Changes which endings are answered, e.g. after the user picks `.test`.
    pub fn set_endings(&self, endings: &[String]) {
        *self.endings.write().unwrap_or_else(|e| e.into_inner()) = normalize(endings);
    }
}

impl Drop for DnsServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// `.Stack` → `stack`.
fn normalize(endings: &[String]) -> Vec<String> {
    endings
        .iter()
        .map(|e| e.trim().trim_matches('.').to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

fn serve(socket: &UdpSocket, endings: &RwLock<Vec<String>>, stop: &AtomicBool) {
    let mut buf = [0u8; 512];
    while !stop.load(Ordering::Relaxed) {
        let Ok((len, from)) = socket.recv_from(&mut buf) else {
            continue;
        };
        let endings = endings.read().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(reply) = answer(&buf[..len], &endings) {
            let _ = socket.send_to(&reply, from);
        }
    }
}

/// Builds the reply to one query, or `None` for packets not worth answering.
pub fn answer(query: &[u8], endings: &[String]) -> Option<Vec<u8>> {
    if query.len() < 12 || query[2] & 0x80 != 0 {
        return None; // too short, or itself a response
    }
    let opcode = (query[2] >> 3) & 0x0f;
    let questions = u16::from_be_bytes([query[4], query[5]]);
    let Some((name, end)) = (opcode == 0 && questions == 1)
        .then(|| read_name(query, 12))
        .flatten()
    else {
        return Some(header_only(query, FORMAT_ERROR));
    };
    if end + 4 > query.len() {
        return Some(header_only(query, FORMAT_ERROR));
    }
    let qtype = u16::from_be_bytes([query[end], query[end + 1]]);
    let qclass = u16::from_be_bytes([query[end + 2], query[end + 3]]);
    let ours = endings
        .iter()
        .any(|e| name == *e || name.ends_with(&format!(".{e}")));
    if !ours {
        return Some(reply(query, end + 4, REFUSED, false));
    }
    let with_address = qclass == CLASS_IN && (qtype == TYPE_A || qtype == TYPE_ANY);
    Some(reply(query, end + 4, NO_ERROR, with_address))
}

/// Reads an uncompressed name; returns it lowercased without the final dot,
/// and the offset just after it.
fn read_name(packet: &[u8], mut at: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    let mut total = 0;
    loop {
        let len = *packet.get(at)? as usize;
        at += 1;
        if len == 0 {
            break;
        }
        if len > 63 {
            return None; // compression or reserved bits: not in a question
        }
        total += len + 1;
        if total > 255 {
            return None;
        }
        let label = packet.get(at..at + len)?;
        labels.push(String::from_utf8_lossy(label).to_ascii_lowercase());
        at += len;
    }
    Some((labels.join("."), at))
}

fn flags(query: &[u8], rcode: u8) -> [u8; 2] {
    // Response, same opcode, authoritative, recursion-desired copied.
    let high = 0x80 | (query[2] & 0x78) | 0x04 | (query[2] & 0x01);
    [high, rcode & 0x0f]
}

fn header_only(query: &[u8], rcode: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    out.extend_from_slice(&query[0..2]);
    out.extend_from_slice(&flags(query, rcode));
    out.extend_from_slice(&[0; 8]);
    out
}

fn reply(query: &[u8], question_end: usize, rcode: u8, with_address: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(question_end + 16);
    out.extend_from_slice(&query[0..2]);
    out.extend_from_slice(&flags(query, rcode));
    out.extend_from_slice(&1u16.to_be_bytes()); // questions
    out.extend_from_slice(&u16::from(with_address).to_be_bytes()); // answers
    out.extend_from_slice(&[0, 0, 0, 0]); // authority, additional
    out.extend_from_slice(&query[12..question_end]);
    if with_address {
        out.extend_from_slice(&[0xc0, 0x0c]); // the name in the question
        out.extend_from_slice(&TYPE_A.to_be_bytes());
        out.extend_from_slice(&CLASS_IN.to_be_bytes());
        out.extend_from_slice(&TTL.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&Ipv4Addr::LOCALHOST.octets());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&CLASS_IN.to_be_bytes());
        q
    }

    fn endings() -> Vec<String> {
        normalize(&[".stack".into(), "Test".into()])
    }

    #[test]
    fn answers_names_under_the_endings() {
        let reply = answer(&query("MyApp.stack", TYPE_A), &endings()).unwrap();
        assert_eq!(&reply[0..2], &[0x12, 0x34]);
        assert_eq!(reply[2] & 0x80, 0x80, "is a response");
        assert_eq!(reply[3] & 0x0f, NO_ERROR);
        assert_eq!(u16::from_be_bytes([reply[6], reply[7]]), 1);
        assert_eq!(&reply[reply.len() - 4..], &[127, 0, 0, 1]);

        let sub = answer(&query("api.shop.test", TYPE_A), &endings()).unwrap();
        assert_eq!(&sub[sub.len() - 4..], &[127, 0, 0, 1]);
    }

    #[test]
    fn has_no_ipv6_address() {
        // Caddy listens on 127.0.0.1 only, so browsers must not try ::1.
        let reply = answer(&query("myapp.stack", 28), &endings()).unwrap();
        assert_eq!(reply[3] & 0x0f, NO_ERROR);
        assert_eq!(u16::from_be_bytes([reply[6], reply[7]]), 0);
    }

    #[test]
    fn refuses_everything_else() {
        for name in ["example.com", "stack.example.com", "notstack"] {
            let reply = answer(&query(name, TYPE_A), &endings()).unwrap();
            assert_eq!(reply[3] & 0x0f, REFUSED, "{name}");
            assert_eq!(u16::from_be_bytes([reply[6], reply[7]]), 0);
        }
    }

    #[test]
    fn copes_with_bad_packets() {
        assert!(answer(&[1, 2, 3], &endings()).is_none());
        let mut truncated = query("myapp.stack", TYPE_A);
        truncated.truncate(20);
        assert_eq!(answer(&truncated, &endings()).unwrap()[3], FORMAT_ERROR);
        let mut response = query("myapp.stack", TYPE_A);
        response[2] |= 0x80;
        assert!(answer(&response, &endings()).is_none());
        let mut pointer = query("myapp.stack", TYPE_A);
        pointer[12] = 0xc0;
        assert_eq!(answer(&pointer, &endings()).unwrap()[3], FORMAT_ERROR);
    }

    #[test]
    fn serves_over_udp() {
        let server = DnsServer::start(0, &[".stack".into()]).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let ask = |name: &str| {
            client
                .send_to(&query(name, TYPE_A), ("127.0.0.1", server.port()))
                .unwrap();
            let mut buf = [0u8; 512];
            let (len, _) = client.recv_from(&mut buf).unwrap();
            buf[..len].to_vec()
        };
        assert_eq!(ask("a.stack")[3] & 0x0f, NO_ERROR);
        server.set_endings(&[".test".into()]);
        assert_eq!(ask("a.stack")[3] & 0x0f, REFUSED);
        assert_eq!(ask("a.test")[3] & 0x0f, NO_ERROR);
        drop(server);
    }
}
