//! Port checks for services.

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

/// `true` when nothing is listening on the port. Stackyard listens on
/// `127.0.0.1`, but another program listening on every address (`0.0.0.0`)
/// or on IPv6 (`::1`, which browsers try first for `localhost`) would still
/// answer some requests, and Windows lets both bind at once, so those count.
pub fn is_free(port: u16) -> bool {
    if TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_err() {
        return false;
    }
    [
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ]
    .into_iter()
    .all(|ip| match TcpListener::bind((ip, port)) {
        Ok(_) => true,
        // Taken, or held exclusively (Windows says "access denied"); any
        // other error means that address isn't there, e.g. no IPv6.
        Err(e) => !matches!(e.kind(), ErrorKind::AddrInUse | ErrorKind::PermissionDenied),
    })
}

/// Waits until something accepts connections on the port, or the timeout passes.
pub fn wait_until_open(port: u16, timeout: Duration) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_listening_port() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!is_free(port));
        assert!(wait_until_open(port, Duration::from_secs(1)));
        drop(listener);
        assert!(is_free(port));
    }

    /// Another local server (Local by Flywheel, for one) listening on every
    /// address or on IPv6 only still takes the port.
    #[test]
    fn counts_other_addresses_too() {
        let any = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        assert!(!is_free(any.local_addr().unwrap().port()));
        if let Ok(v6) = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)) {
            assert!(!is_free(v6.local_addr().unwrap().port()));
        }
    }
}
